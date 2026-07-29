/**
 * Load progress strip along the URL pill's bottom edge.
 *
 * Activation rides two signals: the tab's `loading` flag (state refetch,
 * reliable only on slow loads because refetches coalesce) and the
 * `wis-load-progress` events from the engine poller (macOS/Linux).
 * A cancelled load clears `loading`, so stopping takes the same
 * sweep-and-fade exit. WebView2 exposes no fractional progress, so Windows
 * plays an indeterminate ramp toward 90% off `loading` alone.
 */

import { useEffect, useRef, useState } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import type { Tab } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { currentPlatform } from '@/lib/platform'

const LOAD_PROGRESS_EVENT = 'wis-load-progress'
/** Fill shown the instant a load starts, so the bar reads as begun. */
const START_FRAC = 0.08
/** Time for the ease-to-full transition plus the fade-out. */
const DONE_HIDE_MS = 420
/** Event silence (with `loading` false) after which a load counts as done. */
const QUIET_MS = 300

type Phase = 'idle' | 'active' | 'finishing'

interface ProgressPayload {
  tabId: Tab['id']
  progress: number
}

interface LoadProgressBarProps {
  activeTab: Tab | null
}

export function LoadProgressBar({ activeTab }: LoadProgressBarProps) {
  const [frac, setFrac] = useState(0)
  const [visible, setVisible] = useState(false)

  const id = activeTab?.id ?? null
  const loading = activeTab?.loading ?? false

  const idRef = useRef(id)
  idRef.current = id
  const loadingRef = useRef(loading)
  loadingRef.current = loading

  const phaseRef = useRef<Phase>('idle')
  const lastEventAt = useRef(0)
  const hideTimer = useRef<number | undefined>(undefined)

  // Stable helpers: touch only refs and setState, safe inside [] effects.
  const clearHideTimer = () => {
    if (hideTimer.current !== undefined) {
      window.clearTimeout(hideTimer.current)
      hideTimer.current = undefined
    }
  }
  const begin = () => {
    if (phaseRef.current === 'active') return
    clearHideTimer()
    phaseRef.current = 'active'
    setFrac(START_FRAC)
    setVisible(true)
  }
  const finish = () => {
    if (phaseRef.current !== 'active') return
    phaseRef.current = 'finishing'
    setFrac(1)
    hideTimer.current = window.setTimeout(() => {
      phaseRef.current = 'idle'
      setVisible(false)
      setFrac(0)
      hideTimer.current = undefined
    }, DONE_HIDE_MS)
  }

  // Tab switch: adopt the new tab's state instantly, no cross-tab animation.
  const prevId = useRef(id)
  useEffect(() => {
    if (prevId.current === id) return
    prevId.current = id
    clearHideTimer()
    phaseRef.current = loading ? 'active' : 'idle'
    setVisible(loading)
    setFrac(loading ? START_FRAC : 0)
  })

  // Load start/finish transitions observed through the state flag.
  const prevLoading = useRef(loading)
  useEffect(() => {
    const was = prevLoading.current
    prevLoading.current = loading
    if (loading && !was) begin()
    if (!loading && was) finish()
  })

  // Fractional progress from the engine poller (macOS/Linux). Also the
  // activation path when a fast load's `loading` never reaches the chrome.
  useEffect(() => {
    let cancelled = false
    let unlisten: UnlistenFn | undefined
    void listen<ProgressPayload>(LOAD_PROGRESS_EVENT, (e) => {
      if (e.payload.tabId !== idRef.current) return
      // Stragglers from a finished load must not restart the bar.
      if (phaseRef.current === 'finishing' && !loadingRef.current) return
      lastEventAt.current = Date.now()
      begin()
      const p = Math.min(e.payload.progress, 1)
      setFrac((f) => Math.max(f, p))
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  // Event-activated loads whose `loading` was never observed true complete
  // on event silence instead of a flag transition.
  useEffect(() => {
    if (!visible) return
    const iv = window.setInterval(() => {
      if (phaseRef.current !== 'active') return
      if (loadingRef.current) return
      if (Date.now() - lastEventAt.current > QUIET_MS) finish()
    }, 120)
    return () => window.clearInterval(iv)
  }, [visible])

  // Windows: no engine events; creep toward 90% while loading.
  useEffect(() => {
    if (currentPlatform() !== 'windows' || !loading) return
    const iv = window.setInterval(() => {
      setFrac((f) => Math.min(0.9, f + (0.92 - f) * 0.15))
    }, 180)
    return () => window.clearInterval(iv)
  }, [loading, id])

  useEffect(() => clearHideTimer, [])

  return (
    <div className={cx('kb-urlpill-progress', { 'is-visible': visible })} aria-hidden>
      <div className="kb-urlpill-progress-fill" style={{ transform: `scaleX(${frac})` }} />
    </div>
  )
}
