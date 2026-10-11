/**
 * Chrome-focus keybinding handler for Windows and Linux.
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
 * On Linux no native menu is attached at all (a GTK menu bar packs into the
 * window content box and breaks the webview compositing; see
 * `menu::apply_menu`), so there is no GTK accel-group either and chrome-focused
 * shortcuts need the same JS capture as Windows.
 *
 * On macOS the global menu bar handles shortcuts, except ones that move the
 * caret or delete text (Cmd+Left, Option+Backspace): the menu offers those to
 * this webview first, so they only reach this hook, and nothing fires twice.
 *
 * Scope:
 * - A caret-moving shortcut is left to a focused text field, on every platform.
 * - This webview's focus only. When a tab webview has focus its own keydown
 *   fires in the page, covered by the injected forwarder (`keybind.rs`).
 *
 * Mount exactly once per webview: the chrome mounts it in `Window.tsx` (with
 * `takeFocusOnMount`, see below), and each overlay surface webview mounts it
 * via `OverlayRoot` so shortcuts keep working while a find bar or palette
 * input holds keyboard focus.
 */

import { useEffect, useMemo } from 'react'
import { emit } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import { currentPlatform } from '@/lib/platform'
import {
  isTextEditingKeystroke,
  isTextField,
  keyboardEventToAccelerator,
  normalizeAccelerator,
} from '@/lib/keybindings'
import { effectiveKeymap } from '@/lib/palette'
import type { KeyAction } from '@/bindings/types'

const KEYACTION_EVENT = 'wis-keyaction'

interface KeybindCaptureOptions {
  takeFocusOnMount?: boolean
}

export function useKeybindCapture(
  keybindings: Partial<Record<KeyAction, string>> | undefined,
  opts?: KeybindCaptureOptions,
): void {
  const takeFocusOnMount = opts?.takeFocusOnMount ?? false
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
    if (currentPlatform() === 'macos' || !takeFocusOnMount) return
    void commands.focusSelf().catch(() => undefined)
  }, [takeFocusOnMount])

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent): void => {
      if (
        e.target instanceof HTMLElement &&
        e.target.closest('[data-keybind-capture]')
      ) {
        return
      }
      if (isTextEditingKeystroke(e) && isTextField(e.composedPath()[0] ?? e.target)) return
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
