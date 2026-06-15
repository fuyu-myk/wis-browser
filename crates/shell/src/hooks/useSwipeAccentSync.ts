import { useEffect } from 'react'

import { commands } from '@/bindings/commands'
import type { UserSettings } from '@/bindings/types'

/** Read the live webview-border colour the swipe indicator should match. */
function readBorderColor(): string {
  const get = (el: Element) =>
    getComputedStyle(el).getPropertyValue('--primary-400').trim()
  return get(document.body) || get(document.documentElement)
}

/**
 * Keep the swipe-nav indicator's colour in lockstep with the chrome theme. The
 * indicator lives inside each tab webview, so the chrome reads its resolved `--primary-400`
 * and pushes it to the backend, which bakes it into new tabs and evals it into live ones.
 * Re-runs whenever the theme-relevant settings change, plus on OS light/dark flips.
 * A `requestAnimationFrame` defers the read until after `useThemeApply` has
 * applied the new vars.
 */
export function useSwipeAccentSync(settings: UserSettings | undefined): void {
  const activeTheme = settings?.custom_themes.find((t) => t.id === settings.active_theme)
  const key = settings
    ? [
        settings.theme,
        settings.accent,
        settings.active_theme ?? '',
        activeTheme ? activeTheme.tokens.light.accent : '',
        activeTheme ? activeTheme.tokens.dark.accent : '',
      ].join('|')
    : ''

  useEffect(() => {
    let raf = 0
    const push = () => {
      cancelAnimationFrame(raf)
      raf = requestAnimationFrame(() => {
        const color = readBorderColor()
        if (color) void commands.setSwipeAccent(color).catch(() => undefined)
      })
    }
    push()
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    mq.addEventListener('change', push)
    return () => {
      cancelAnimationFrame(raf)
      mq.removeEventListener('change', push)
    }
  }, [key])
}
