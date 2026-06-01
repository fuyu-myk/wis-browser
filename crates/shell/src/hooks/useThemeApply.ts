/**
 * Side-effect hook that mirrors selected `UserSettings` onto the
 * `<body>` element via `data-*` attributes and class toggles. Centralised
 * so the CSS selectors in `tokens.css` / `window.css` have a single
 * source of attributes.
 *
 * Also reacts to OS-level light/dark changes when `theme === 'system'`.
 */

import { useEffect } from 'react'

import type { UserSettings } from '@/bindings/types'

export function useThemeApply(settings: UserSettings | undefined) {
  useEffect(() => {
    if (!settings) return
    const body = document.body

    const applyMode = () => {
      const mode =
        settings.theme === 'system'
          ? window.matchMedia('(prefers-color-scheme: dark)').matches
            ? 'dark'
            : 'light'
          : settings.theme
      body.setAttribute('data-mode', mode)
    }
    applyMode()

    body.setAttribute('data-accent', settings.accent)
    body.classList.toggle('glass-on', settings.glass_enabled)
    body.classList.toggle('compact', settings.compact)
    body.classList.toggle('url-glow-on', settings.url_bar_focus_glow)

    if (settings.theme !== 'system') return
    const media = window.matchMedia('(prefers-color-scheme: dark)')
    const onChange = () => applyMode()
    media.addEventListener('change', onChange)
    return () => media.removeEventListener('change', onChange)
  }, [settings])
}
