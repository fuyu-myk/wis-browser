import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { TabId } from '@/bindings/types'
import { currentPlatform } from '@/lib/platform'
import { guardFeatureUnavailable, isFeatureUnavailableHere } from '@/lib/platformSupport'
import {
  loadPopoutGeometry,
  POPOUT_CLOSED_EVENT,
  POPOUT_MEDIA_EVENT,
  POPOUT_OPENED_EVENT,
  POPOUT_REQUESTED_EVENT,
  savePopoutGeometry,
  type PopoutClosedPayload,
  type PopoutMediaPayload,
  type PopoutOpenedPayload,
  type PopoutRequestedPayload,
} from '@/lib/videoPopout'

/** The enter handshake should resolve well within this; past it the page has
 *  no usable video (or is hung) and the adoption is abandoned. */
const PENDING_TIMEOUT_MS = 4000

type PopoutPhase = 'pending' | 'open' | 'closing'

/** How the popout came to be: `auto` (tab playing video left the layout;
 *  folds back when the tab returns) or `manual` (hover button / key action). */
type PopoutMode = 'auto' | 'manual'

interface PopoutSession {
  tabId: TabId
  phase: PopoutPhase
  mode: PopoutMode
}

/**
 * Orchestration handle consumed by `useTabWebviews` and the pane host:
 * decides which tab becomes the floating mini player and folds it back.
 */
export interface VideoPopoutController {
  /** True while `id` is popped out (any phase). Such a tab's webview is
   *  natively managed by the popout: never sleep it, never re-hide it,
   *  never re-bound it, and ignore its pane-focus clicks. */
  isPopout: (id: TabId) => boolean
  /** A webview just went hidden; adopt it automatically when it qualifies
   *  (feature on, video playing, no popout live yet). */
  onHidden: (id: TabId) => void
  /** `id` is in the visible layout while popped: an auto popout folds back
   *  (the user returned to the tab); a manual one keeps floating behind its
   *  pane placeholder. */
  onVisible: (id: TabId) => void
  /** Explicitly pop `id` out (hover button / key action); toggles back when
   *  `id` is already the popout. Gated on the page reporting a usable video. */
  requestPopOut: (id: TabId) => void
  /** Fold the popout back in. The registered reconcile callback re-shows the
   *  webview once the native restore lands. */
  release: (id: TabId) => void
  /** `id`'s tab is being closed outright. */
  noteClosed: (id: TabId) => void
  /** Register the callback that re-reconciles a tab after its restore. */
  setReconcile: (fn: (id: TabId) => void) => void
}

export interface VideoPopoutHandle {
  controller: VideoPopoutController
  /** The popped-out tab, for the pane placeholder. Null when none. */
  poppedTabId: TabId | null
}

function labelFor(id: TabId): string {
  return `tab-${id}`
}

/**
 * Video popout state machine. Owns no DOM beyond the pane placeholder flag:
 * the player is a separate native always-on-top window (`video_popout.rs`);
 * this hook reacts to the native events, gates the automatic trigger on the
 * playing-state map (and the manual ones on the present map) reported by the
 * injected page script, and persists the remembered geometry.
 */
export function useVideoPopout({ enabled }: { enabled: boolean }): VideoPopoutHandle {
  const enabledRef = useRef(enabled)
  enabledRef.current = enabled
  // Tabs currently playing a qualifying (unmuted, big-enough) video.
  const playingRef = useRef<Set<TabId>>(new Set())
  // Tabs with any usable video, playing or paused (manual-trigger gate).
  const presentRef = useRef<Set<TabId>>(new Set())
  const sessionRef = useRef<PopoutSession | null>(null)
  const [poppedTabId, setPoppedTabId] = useState<TabId | null>(null)
  const pendingTimerRef = useRef<number | null>(null)
  const reconcileRef = useRef<((id: TabId) => void) | null>(null)

  const setSession = useCallback((next: PopoutSession | null) => {
    sessionRef.current = next
    setPoppedTabId(next?.tabId ?? null)
  }, [])

  const clearPendingTimer = useCallback(() => {
    if (pendingTimerRef.current !== null) {
      window.clearTimeout(pendingTimerRef.current)
      pendingTimerRef.current = null
    }
  }, [])

  const isPopout = useCallback(
    (id: TabId) => sessionRef.current?.tabId === id,
    [],
  )

  const open = useCallback(
    (id: TabId, mode: PopoutMode) => {
      if (isFeatureUnavailableHere('pop_out_video')) {
        if (mode === 'manual') guardFeatureUnavailable('pop_out_video')
        return
      }
      // Windows uses the engine's native Picture-in-Picture.
      if (currentPlatform() === 'windows') {
        if (mode === 'manual') {
          void commands.openVideoPopout(labelFor(id), loadPopoutGeometry()).catch(() => undefined)
        }
        return
      }
      setSession({ tabId: id, phase: 'pending', mode })
      pendingTimerRef.current = window.setTimeout(() => {
        pendingTimerRef.current = null
        const session = sessionRef.current
        if (session?.tabId !== id || session.phase !== 'pending') return
        // No enter confirmation: the page had no usable video or is hung.
        // The cancel also unwinds any styling the page half-applied.
        setSession(null)
        void commands.closeVideoPopout(labelFor(id)).catch(() => undefined)
      }, PENDING_TIMEOUT_MS)

      void commands.openVideoPopout(labelFor(id), loadPopoutGeometry()).catch(() => {
        if (sessionRef.current?.tabId === id) {
          setSession(null)
          clearPendingTimer()
        }
      })
    },
    [setSession, clearPendingTimer],
  )

  const onHidden = useCallback(
    (id: TabId) => {
      if (!enabledRef.current) return
      if (sessionRef.current !== null) return
      if (!playingRef.current.has(id)) return
      open(id, 'auto')
    },
    [open],
  )

  const release = useCallback(
    (id: TabId) => {
      const session = sessionRef.current
      if (session?.tabId !== id || session.phase === 'closing') return
      sessionRef.current = { ...session, phase: 'closing' }
      clearPendingTimer()
      void commands.closeVideoPopout(labelFor(id)).catch(() => {
        if (sessionRef.current?.tabId === id) {
          setSession(null)
          reconcileRef.current?.(id)
        }
      })
    },
    [setSession, clearPendingTimer],
  )

  const onVisible = useCallback(
    (id: TabId) => {
      const session = sessionRef.current
      if (session?.tabId !== id) return
      if (session.mode === 'auto') release(id)
    },
    [release],
  )

  const requestPopOut = useCallback(
    (id: TabId) => {
      const session = sessionRef.current
      if (session) {
        // Toggle semantics: re-triggering on the popped tab folds it back.
        if (session.tabId === id) release(id)
        return
      }
      // Windows posts no media reports.
      if (
        currentPlatform() !== 'windows' &&
        !presentRef.current.has(id) &&
        !playingRef.current.has(id)
      )
        return
      open(id, 'manual')
    },
    [open, release],
  )

  const noteClosed = useCallback(
    (id: TabId) => {
      playingRef.current.delete(id)
      presentRef.current.delete(id)
      release(id)
    },
    [release],
  )

  const setReconcile = useCallback((fn: (id: TabId) => void) => {
    reconcileRef.current = fn
  }, [])

  useEffect(() => {
    let cancelled = false
    const unlistens: UnlistenFn[] = []
    const track = (p: Promise<UnlistenFn>) => {
      void p.then((fn) => {
        if (cancelled) fn()
        else unlistens.push(fn)
      })
    }

    track(
      listen<PopoutMediaPayload>(POPOUT_MEDIA_EVENT, (e) => {
        const id = e.payload.tab as TabId
        if (e.payload.playing) playingRef.current.add(id)
        else playingRef.current.delete(id)
        if (e.payload.present) presentRef.current.add(id)
        else presentRef.current.delete(id)
      }),
    )

    track(
      listen<PopoutRequestedPayload>(POPOUT_REQUESTED_EVENT, (e) => {
        requestPopOut(e.payload.tab as TabId)
      }),
    )

    track(
      listen<PopoutOpenedPayload>(POPOUT_OPENED_EVENT, (e) => {
        const session = sessionRef.current
        if (session?.tabId !== (e.payload.tab as TabId)) return
        if (session.phase === 'pending') {
          clearPendingTimer()
          sessionRef.current = { ...session, phase: 'open' }
        }
      }),
    )

    track(
      listen<PopoutClosedPayload>(POPOUT_CLOSED_EVENT, (e) => {
        const id = e.payload.tab as TabId
        if (sessionRef.current?.tabId !== id) return
        const { x, y } = e.payload
        if (x !== undefined && y !== undefined) {
          savePopoutGeometry({ x, y })
        }
        setSession(null)
        clearPendingTimer()
        reconcileRef.current?.(id)
      }),
    )

    return () => {
      cancelled = true
      unlistens.forEach((fn) => fn())
      clearPendingTimer()
    }
  }, [requestPopOut, setSession, clearPendingTimer])

  // Turning the setting off folds an automatic popout back in; a manual one
  // was an explicit request and is unaffected by the auto toggle.
  useEffect(() => {
    if (enabled) return
    const session = sessionRef.current
    if (session?.mode === 'auto') release(session.tabId)
  }, [enabled, release])

  const controller = useMemo(
    () => ({
      isPopout,
      onHidden,
      onVisible,
      requestPopOut,
      release,
      noteClosed,
      setReconcile,
    }),
    [isPopout, onHidden, onVisible, requestPopOut, release, noteClosed, setReconcile],
  )

  return useMemo(() => ({ controller, poppedTabId }), [controller, poppedTabId])
}
