/**
 * The latest request to ask the assistant about something, or null.
 *
 * Requests are broadcast to every window, so anything for another window is
 * dropped here.
 */

import { useEffect, useState } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { AI_ASK_EVENT, type AiAskRequest } from '@/lib/aiAsk'

export function useAiAsk(): AiAskRequest | null {
  const [request, setRequest] = useState<AiAskRequest | null>(null)

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<AiAskRequest>(AI_ASK_EVENT, (event) => {
      if (event.payload.window !== getCurrentWindow().label) return
      setRequest(event.payload)
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  return request
}
