/**
 * Windows-only chrome-focus keybinding handler.
 *
 * On Windows the native menu accelerators never fire: a focused WebView2
 * processes input out-of-process, so the keystroke never reaches the frame
 * window's accelerator table (Tauri's `TranslateAcceleratorW` msg-hook), and
 * the vendored wry does not forward it via `AcceleratorKeyPressed`. Since the
 * chrome itself is a WebView2, app shortcuts would be dead. This hook makes the
 * chrome handle its own shortcuts and emit the very same `wis-keyaction` event
 * the native menu would, so every existing listener (`useKeyActions`, the find
 * and palette overlay surfaces) works unchanged.
 *
 * Scope:
 * - Windows only. macOS dispatches via the global menu bar and Linux via GTK's
 *   window accel-group (which fires through webview focus and survives the
 *   menu-bar hide), so running this there would double-fire each action.
 * - Chrome focus only. When a tab webview has focus its own keydown fires in
 *   the page, not here; that path is covered by an injected forwarder.
 *
 * Mount exactly once (see `Window.tsx`), like `useKeyActions`.
 */

import { useEffect, useMemo } from 'react'
import { emit } from '@tauri-apps/api/event'

import { currentPlatform } from '@/lib/platform'
import {
  keyboardEventToAccelerator,
  normalizeAccelerator,
} from '@/lib/keybindings'
import { effectiveKeymap } from '@/lib/palette'
import type { KeyAction } from '@/bindings/types'

const KEYACTION_EVENT = 'wis-keyaction'

export function useKeybindCapture(
  keybindings: Partial<Record<KeyAction, string>> | undefined,
): void {
  // Reverse the effective keymap into normalized-accelerator -> action. Rebuilt
  // only when the user's overrides change, not every render.
  const lookup = useMemo(() => {
    const map = new Map<string, KeyAction>()
    for (const [action, accel] of Object.entries(effectiveKeymap(keybindings))) {
      if (accel) map.set(normalizeAccelerator(accel), action as KeyAction)
    }
    return map
  }, [keybindings])

  useEffect(() => {
    if (currentPlatform() !== 'windows') return

    const onKeyDown = (e: KeyboardEvent): void => {
      if (
        e.target instanceof HTMLElement &&
        e.target.closest('[data-keybind-capture]')
      ) {
        return
      }
      const accel = keyboardEventToAccelerator(e)
      if (!accel) return
      const action = lookup.get(normalizeAccelerator(accel))
      if (!action) return
      e.preventDefault()
      e.stopPropagation()
      void emit(KEYACTION_EVENT, { action })
    }

    window.addEventListener('keydown', onKeyDown, { capture: true })
    return () => window.removeEventListener('keydown', onKeyDown, { capture: true })
  }, [lookup])
}
