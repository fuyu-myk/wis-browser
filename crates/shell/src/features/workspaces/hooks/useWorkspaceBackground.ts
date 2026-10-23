/**
 * Applies the active workspace's custom background by overriding the
 * `--wp-grad` token on `<body>` (it already drives the launchpad page), so a
 * gradient or wallpaper themes the page with zero component changes. Images
 * additionally set fit/blur vars consumed by the launchpad's backdrop layer.
 * Cleared when the workspace has no custom background.
 */

import { useEffect } from 'react'

import type { Workspace } from '@/bindings/types'
import { backgroundCss, fitCss } from '@/features/workspaces/lib/background'

export function useWorkspaceBackground(workspace: Workspace | null | undefined): void {
  const bg = workspace?.background ?? null
  const css = bg ? backgroundCss(bg) : null
  const image = bg && 'image' in bg ? bg.image : null
  const fit = image ? fitCss(image.fit) : null
  const fitSize = fit?.size
  const fitRepeat = fit?.repeat
  const blur = image ? Math.min(40, Math.max(0, image.blur)) : 0

  useEffect(() => {
    const body = document.body
    const clear = () => {
      body.style.removeProperty('--wp-grad')
      body.style.removeProperty('--wp-bg-size')
      body.style.removeProperty('--wp-bg-repeat')
      body.style.removeProperty('--wp-blur')
      body.classList.remove('has-custom-bg', 'has-custom-img')
    }
    if (!css) {
      clear()
      return clear
    }
    body.style.setProperty('--wp-grad', css)
    // Liquid glass makes the launchpad transparent; a custom background
    // explicitly set by the user wins over that (see window.css).
    body.classList.add('has-custom-bg')
    if (fitSize !== undefined && fitRepeat !== undefined) {
      body.style.setProperty('--wp-bg-size', fitSize)
      body.style.setProperty('--wp-bg-repeat', fitRepeat)
      body.style.setProperty('--wp-blur', `${blur}px`)
      body.classList.add('has-custom-img')
    } else {
      body.style.removeProperty('--wp-bg-size')
      body.style.removeProperty('--wp-bg-repeat')
      body.style.removeProperty('--wp-blur')
      body.classList.remove('has-custom-img')
    }
    return clear
  }, [css, fitSize, fitRepeat, blur])
}
