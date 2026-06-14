import { useEffect } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { commands } from '@/bindings/commands'

/**
 * Reopen the saved multi-window layout on launch. Only the `main` window drives
 * this: it asks the backend to spawn a window for each workspace that had one
 * last session. The backend is itself guarded to run once per process, so a
 * reload of the main chrome is harmless. Best-effort and fire-and-forget.
 */
export function useSessionRestore(): void {
  useEffect(() => {
    let label: string
    try {
      label = getCurrentWindow().label
    } catch {
      return
    }
    if (label !== 'main') return
    void commands.restoreSessionWindows().catch(() => undefined)
  }, [])
}
