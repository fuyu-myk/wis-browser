/**
 * Mounted by `main.tsx` when the webview label starts with `overlay-`.
 * Each child webview's HTML loads the same bundle; this router picks the
 * right surface for the webview's purpose. Unknown surfaces render
 * nothing (the overlay stays transparent and click-through).
 */

import { FindSurface } from '@/overlay/surfaces/FindSurface'
import { PaletteSurface } from '@/overlay/surfaces/PaletteSurface'

interface OverlayRootProps {
  surface: string
}

export function OverlayRoot({ surface }: OverlayRootProps) {
  if (surface === 'find') return <FindSurface />
  if (surface === 'palette') return <PaletteSurface />
  // eslint-disable-next-line no-console
  console.warn(`Unknown overlay surface: ${surface}`)
  return null
}
