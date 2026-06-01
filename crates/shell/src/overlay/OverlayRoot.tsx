/**
 * Mounted by `main.tsx` when the URL carries `?surface=<name>`. Each child
 * webview's HTML loads the same bundle; this router picks the right surface
 * for the webview's purpose. Unknown surfaces render nothing (the overlay
 * stays transparent and click-through).
 */

import { FindSurface } from '@/overlay/surfaces/FindSurface'

interface OverlayRootProps {
  surface: string
}

export function OverlayRoot({ surface }: OverlayRootProps) {
  switch (surface) {
    case 'find':
      return <FindSurface />
    default:
      // eslint-disable-next-line no-console
      console.warn(`Unknown overlay surface: ${surface}`)
      return null
  }
}
