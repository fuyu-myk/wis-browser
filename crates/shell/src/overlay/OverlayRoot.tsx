/**
 * Mounted by `main.tsx` when the webview label starts with `overlay-`.
 * Each child webview's HTML loads the same bundle; this router picks the
 * right surface for the webview's purpose. Unknown surfaces render
 * nothing (the overlay stays transparent and click-through).
 */

import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useKeybindCapture } from '@/hooks/useKeybindCapture'
import { ClearDataSurface } from '@/features/privacy/surfaces/ClearDataSurface'
import { DownloadsMenuSurface } from '@/features/downloads/DownloadsMenuSurface'
import { ExtensionsMenuSurface } from '@/features/extensions/surfaces/ExtensionsMenuSurface'
import { FindSurface } from '@/features/page-tools/FindSurface'
import { BookmarkPickerSurface } from '@/features/bookmarks/surfaces/BookmarkPickerSurface'
import { BookmarksMenuSurface } from '@/features/bookmarks/surfaces/BookmarksMenuSurface'
import { IslandMenuSurface } from '@/features/tabs/surfaces/IslandMenuSurface'
import { NavGuardSurface } from '@/features/page-prompts/NavGuardSurface'
import { PaletteSurface } from '@/features/keyboard/PaletteSurface'
import { TabContextMenuSurface } from '@/features/tabs/surfaces/TabContextMenuSurface'
import { PageMenuSurface } from '@/features/page-tools/PageMenuSurface'
import { TabPreviewSurface } from '@/features/tabs/surfaces/TabPreviewSurface'
import { PeekSurface } from '@/features/page-tools/PeekSurface'
import { AppLaunchSurface } from '@/features/page-prompts/AppLaunchSurface'
import { SavedLoginsSurface } from '@/features/vault/surfaces/SavedLoginsSurface'
import { AddressCardSurface } from '@/features/vault/surfaces/AddressCardSurface'
import { CardPickerSurface } from '@/features/vault/surfaces/CardPickerSurface'
import { SignupOfferSurface } from '@/features/vault/surfaces/SignupOfferSurface'
import { SitePanelSurface } from '@/features/site/surfaces/SitePanelSurface'
import { PermissionSurface } from '@/features/page-prompts/PermissionSurface'
import { ScreenshotRegionSurface } from '@/features/screenshot/ScreenshotRegionSurface'
import { ScreenshotToastSurface } from '@/features/screenshot/ScreenshotToastSurface'
import { ToastSurface } from '@/overlay/ToastSurface'
import { UrlbarSuggestSurface } from '@/features/urlbar/UrlbarSuggestSurface'
import { VaultMenuSurface } from '@/features/vault/surfaces/VaultMenuSurface'
import { WidgetHeaderSurface } from '@/features/widgets/WidgetHeaderSurface'
import { WidgetResizeSurface } from '@/features/widgets/WidgetResizeSurface'
import { WindowPickerSurface } from '@/features/workspaces/surfaces/WindowPickerSurface'

interface OverlayRootProps {
  surface: string
}

export function OverlayRoot({ surface }: OverlayRootProps) {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useKeybindCapture(state?.settings.keybindings)

  if (surface === 'find') return <FindSurface />
  if (surface === 'palette') return <PaletteSurface />
  if (surface === 'tabmenu') return <TabContextMenuSurface />
  if (surface === 'pagemenu') return <PageMenuSurface />
  if (surface === 'tabpreview') return <TabPreviewSurface />
  if (surface === 'islandmenu') return <IslandMenuSurface />
  if (surface === 'extmenu') return <ExtensionsMenuSurface />
  if (surface === 'site') return <SitePanelSurface />
  if (surface === 'cleardata') return <ClearDataSurface />
  if (surface === 'urlbar') return <UrlbarSuggestSurface />
  if (surface === 'vaultmenu') return <VaultMenuSurface />
  if (surface === 'peek') return <PeekSurface />
  if (surface === 'navguard') return <NavGuardSurface />
  if (surface === 'permission') return <PermissionSurface />
  if (surface === 'signup') return <SignupOfferSurface />
  if (surface === 'logins') return <SavedLoginsSurface />
  if (surface === 'addresses') return <AddressCardSurface />
  if (surface === 'cards') return <CardPickerSurface />
  if (surface === 'applaunch') return <AppLaunchSurface />
  if (surface === 'bookmarks') return <BookmarksMenuSurface />
  if (surface === 'downloads') return <DownloadsMenuSurface />
  if (surface === 'bmpicker') return <BookmarkPickerSurface />
  if (surface === 'widgetresize') return <WidgetResizeSurface />
  if (surface === 'widgetheader') return <WidgetHeaderSurface />
  if (surface === 'windowpicker') return <WindowPickerSurface />
  if (surface === 'screenshot-toast') return <ScreenshotToastSurface />
  if (surface === 'screenshot-region') return <ScreenshotRegionSurface />
  if (surface === 'toast') return <ToastSurface />
  // eslint-disable-next-line no-console
  console.warn(`Unknown overlay surface: ${surface}`)
  return null
}
