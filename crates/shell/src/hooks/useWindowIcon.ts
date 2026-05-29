import { useCallback, useRef } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { commands } from '@/bindings/commands'

/**
 * Drive the OS window/dock icon between the `open` and `closed` owl variants.
 * The hook debounces redundant calls so rapid hover in/out doesn't fire a
 * cascade of IPC. Failures are swallowed: in the browser dev preview the
 * Tauri APIs throw, and the icon swap is a best-effort cosmetic.
 */
export function useWindowIconSwap(): (variant: 'open' | 'closed') => void {
  const last = useRef<'open' | 'closed' | null>(null)

  return useCallback((variant) => {
    if (last.current === variant) return
    last.current = variant
    let label: string
    try {
      label = getCurrentWindow().label
    } catch {
      return
    }
    commands.setWindowIcon(label, variant).catch(() => {
      // Reset so a future call can retry.
      last.current = null
    })
  }, [])
}
