import { useEffect, useRef } from 'react'
import { listen } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { TabId } from '@/bindings/types'
import { useEscapeKey, focusSurface } from '@/hooks/useSurfaceFocus'

/** `event` is the field script's pick event: `ADDRESS_PICK_EVENT` or `FIELD_PICK_EVENT`. */
export function usePickerKeys(event: string, tabId: TabId | null, close: () => void): void {
  const tabRef = useRef(tabId)
  tabRef.current = tabId

  useEffect(() => {
    let cancelled = false
    let unlisten: (() => void) | null = null
    void listen<{ tabId: TabId }>(event, (picked) => {
      if (picked.payload.tabId === tabRef.current) focusSurface()
    }).then((fn) => (cancelled ? fn() : (unlisten = fn)))
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [event])

  useEscapeKey(tabId !== null, () => {
    const tab = tabRef.current
    close()
    if (tab) void commands.focusWebview(`tab-${tab}`).catch(() => undefined)
  })
}
