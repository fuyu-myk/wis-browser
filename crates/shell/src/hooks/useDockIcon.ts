import { useEffect } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { commands } from '@/bindings/commands'
import { isWindowFocused } from '@/lib/windowFocus'

/**
 * Keep the process-global macOS Dock icon in sync with the *focused* window's
 * privacy state.
 *
 * The Dock icon is one per app, but the browser is one workspace per window, so
 * a window only drives it while it holds focus: the window that gains focus
 * claims the icon for its own privacy state (and re-claims it whenever its
 * workspace's privacy flips while focused). Best-effort: failures (e.g. the dev
 * preview where the Tauri API throws) are swallowed.
 */
export function useDockIcon(isPrivate: boolean): void {
  useEffect(() => {
    let unlisten: (() => void) | undefined
    const apply = () =>
      void commands.setDockIcon(isPrivate ? 'closed' : 'open').catch(() => undefined)

    // Claim the icon now if already focused, and again on every focus-gain.
    if (isWindowFocused()) apply()
    void getCurrentWindow()
      .onFocusChanged(({ payload: focused }) => {
        if (focused) apply()
      })
      .then((fn) => {
        unlisten = fn
      })
      .catch(() => undefined)

    return () => unlisten?.()
  }, [isPrivate])
}
