import { useCallback, useEffect, useRef, useState } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { TabId } from '@/bindings/types'

/** Snapshot broadcast whenever a tab's audibility or mute state changes. */
const TAB_AUDIO_EVENT = 'wis-tab-audio'

interface TabAudioPayload {
  audible: string[]
  muted: string[]
}

export interface TabAudio {
  /** Tabs whose webview is audibly playing right now. */
  audible: ReadonlySet<TabId>
  /** Tabs the user muted this session. */
  muted: ReadonlySet<TabId>
  /** Flip a tab's mute, optimistically (the native snapshot reconciles). */
  toggleMute: (id: TabId) => void
}

/**
 * Per-tab audio state for the tab strips: audibility comes from the native
 * poll (`tabs/audio.rs`), mute is engine-level page muting toggled per tab.
 */
export function useTabAudio(): TabAudio {
  const [audible, setAudible] = useState<ReadonlySet<TabId>>(new Set())
  const [muted, setMuted] = useState<ReadonlySet<TabId>>(new Set())
  const mutedRef = useRef(muted)
  mutedRef.current = muted

  useEffect(() => {
    let cancelled = false
    let unlisten: UnlistenFn | null = null

    const apply = (s: TabAudioPayload) => {
      setAudible(new Set(s.audible as TabId[]))
      setMuted(new Set(s.muted as TabId[]))
    }

    void commands
      .tabAudioState()
      .then((s) => {
        if (!cancelled) apply(s)
      })
      .catch(() => undefined)
    void listen<TabAudioPayload>(TAB_AUDIO_EVENT, (e) => apply(e.payload)).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })

    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  const toggleMute = useCallback((id: TabId) => {
    const next = !mutedRef.current.has(id)
    setMuted((prev) => {
      const out = new Set(prev)
      if (next) out.add(id)
      else out.delete(id)
      return out
    })
    void commands.setTabMuted(id, next).catch(() => undefined)
  }, [])

  return { audible, muted, toggleMute }
}
