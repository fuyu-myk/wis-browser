import { useEffect, useState } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { AiSettings } from '@/bindings/types'
import { aiActivated } from '@/features/ai/lib/ai'
import { isFeatureUnavailableHere } from '@/lib/platformSupport'

/** Tauri event broadcast when the stored provider key is set or cleared. */
const AI_KEY_EVENT = 'wis-ai-key-changed'

/**
 * Whether the assistant is offered at all: the settings name somewhere to send
 * to, with a stored key where one is needed.
 *
 * The key event keeps every window in step with a key saved in any of them.
 */
export function useAiAvailable(ai: AiSettings | undefined): boolean {
  const canStoreKey = !isFeatureUnavailableHere('ai_api_key')
  const [keyPresent, setKeyPresent] = useState(false)

  useEffect(() => {
    if (!canStoreKey) return
    let cancelled = false
    let unlisten: UnlistenFn | null = null

    void commands.aiKeyPresent().then(
      (value) => {
        if (!cancelled) setKeyPresent(value)
      },
      () => undefined,
    )

    void listen<boolean>(AI_KEY_EVENT, (e) => setKeyPresent(e.payload)).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })

    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [canStoreKey])

  return ai !== undefined && aiActivated(ai, keyPresent)
}
