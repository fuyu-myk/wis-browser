/**
 * Applies the active workspace's custom background by overriding the
 * `--wp-grad` token on `<body>` (it already drives the launchpad page), so a
 * gradient themes the page with zero component changes. Cleared when the
 * workspace has no custom background, falling back to the theme default.
 */

import { useEffect } from 'react'

import type { Workspace } from '@/bindings/types'
import { backgroundCss } from '@/lib/background'

export function useWorkspaceBackground(workspace: Workspace | null | undefined): void {
  const bg = workspace?.background ?? null
  const css = bg ? backgroundCss(bg) : null

  useEffect(() => {
    const body = document.body
    if (css) {
      body.style.setProperty('--wp-grad', css)
      // Liquid glass makes the launchpad transparent; a custom background
      // explicitly set by the user wins over that (see window.css).
      body.classList.add('has-custom-bg')
    } else {
      body.style.removeProperty('--wp-grad')
      body.classList.remove('has-custom-bg')
    }
    return () => {
      body.style.removeProperty('--wp-grad')
      body.classList.remove('has-custom-bg')
    }
  }, [css])
}
