/**
 * Side-effect hook that mirrors selected `UserSettings` onto the
 * `<body>` element via `data-*` attributes and class toggles. Centralised
 * so the CSS selectors in `tokens.css` / `window.css` have a single
 * source of attributes.
 *
 * Custom themes: when `active_theme` is set, the theme's tokens for the
 * effective mode are injected as inline CSS variables on `<body>` (inline
 * wins over the `[data-accent]`/`[data-mode]` rules), so the whole chrome
 * re-themes with zero component changes. Cleared when no theme is active.
 *
 * Also reacts to OS-level light/dark changes when `theme === 'system'`.
 */

import { useEffect } from 'react'

import type { UserSettings } from '@/bindings/types'
import { themeVars, THEME_VAR_KEYS } from '@/lib/theme'

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

      const active = settings.active_theme
        ? settings.custom_themes.find((t) => t.id === settings.active_theme)
        : undefined
      if (active) {
        const vars = themeVars(mode === 'dark' ? active.tokens.dark : active.tokens.light)
        for (const [key, value] of Object.entries(vars)) {
          body.style.setProperty(key, value)
        }
      } else {
        for (const key of THEME_VAR_KEYS) body.style.removeProperty(key)
      }
    }
    applyMode()

    body.setAttribute('data-accent', settings.accent)
    body.classList.toggle('glass-on', settings.glass_enabled)
    body.classList.toggle('compact', settings.compact)
    body.classList.toggle('url-glow-on', settings.url_bar_focus_glow)

    // Motion tier: the OS reduce-motion preference caps Full at Reduced.
    const reduceMedia = window.matchMedia('(prefers-reduced-motion: reduce)')
    const applyMotion = () => {
      const effective =
        settings.motion === 'full' && reduceMedia.matches ? 'reduced' : settings.motion
      body.classList.toggle('motion-reduced', effective === 'reduced')
      body.classList.toggle('motion-off', effective === 'off')
    }
    applyMotion()
    reduceMedia.addEventListener('change', applyMotion)

    const themeMedia = window.matchMedia('(prefers-color-scheme: dark)')
    const onThemeChange = () => applyMode()
    if (settings.theme === 'system') {
      themeMedia.addEventListener('change', onThemeChange)
    }
    return () => {
      reduceMedia.removeEventListener('change', applyMotion)
      themeMedia.removeEventListener('change', onThemeChange)
    }
  }, [settings])
}
