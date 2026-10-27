import { useEffect, useRef } from 'react'

import { commands } from '@/bindings/commands'
import { firstStop } from '@/lib/tabFocus'

/**
 * Take keyboard focus for this overlay webview and put it on the surface's
 * `[data-autofocus]` control, or its first control. On close, `hide_overlay`
 * hands focus back to where it was.
 */
export function focusSurface(): void {
  void commands
    .focusSelf()
    .catch(() => undefined)
    .then(() =>
      requestAnimationFrame(() => {
        const target = document.querySelector<HTMLElement>('[data-autofocus]') ?? firstStop(document)
        target?.focus()
      }),
    )
}

/** Focus the surface each time `opened` turns truthy or changes to a new payload. */
export function useSurfaceFocus(opened: unknown): void {
  useEffect(() => {
    if (opened) focusSurface()
  }, [opened])
}

/** Escape runs `onEscape` while `active`. */
export function useEscapeKey(active: boolean, onEscape: () => void): void {
  const handler = useRef(onEscape)
  handler.current = onEscape
  useEffect(() => {
    if (!active) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || e.defaultPrevented) return
      e.preventDefault()
      handler.current()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [active])
}
