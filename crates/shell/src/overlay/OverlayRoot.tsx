/**
 * Mounted by `main.tsx` when the webview label starts with `overlay-`.
 * Each child webview's HTML loads the same bundle; this router picks the
 * right surface for the webview's purpose. Unknown surfaces render
 * nothing (the overlay stays transparent and click-through).
 */

import { ClearDataSurface } from '@/overlay/surfaces/ClearDataSurface'
import { FindSurface } from '@/overlay/surfaces/FindSurface'
import { BookmarkPickerSurface } from '@/overlay/surfaces/BookmarkPickerSurface'
import { BookmarksMenuSurface } from '@/overlay/surfaces/BookmarksMenuSurface'
import { IslandMenuSurface } from '@/overlay/surfaces/IslandMenuSurface'
import { NavGuardSurface } from '@/overlay/surfaces/NavGuardSurface'
import { PaletteSurface } from '@/overlay/surfaces/PaletteSurface'
import { PaneSplitMenuSurface } from '@/overlay/surfaces/PaneSplitMenuSurface'
import { PeekSurface } from '@/overlay/surfaces/PeekSurface'
import { UrlbarSuggestSurface } from '@/overlay/surfaces/UrlbarSuggestSurface'
import { VaultMenuSurface } from '@/overlay/surfaces/VaultMenuSurface'
import { WidgetResizeSurface } from '@/overlay/surfaces/WidgetResizeSurface'

interface OverlayRootProps {
  surface: string
}

export function OverlayRoot({ surface }: OverlayRootProps) {
  if (surface === 'find') return <FindSurface />
  if (surface === 'palette') return <PaletteSurface />
  if (surface === 'tabmenu') return <PaneSplitMenuSurface />
  if (surface === 'islandmenu') return <IslandMenuSurface />
  if (surface === 'cleardata') return <ClearDataSurface />
  if (surface === 'urlbar') return <UrlbarSuggestSurface />
  if (surface === 'vaultmenu') return <VaultMenuSurface />
  if (surface === 'peek') return <PeekSurface />
  if (surface === 'navguard') return <NavGuardSurface />
  if (surface === 'bookmarks') return <BookmarksMenuSurface />
  if (surface === 'bmpicker') return <BookmarkPickerSurface />
  if (surface === 'widgetresize') return <WidgetResizeSurface />
  // eslint-disable-next-line no-console
  console.warn(`Unknown overlay surface: ${surface}`)
  return null
}
