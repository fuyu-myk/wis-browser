/**
 * Window control buttons (minimize / maximize-restore / close) for platforms
 * without native controls in the client area. Windows and Linux run
 * borderless (`decorations: false`) with the chrome drawing its own top bar.
 */

import { useEffect, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { currentPlatform } from '@/lib/platform'
import { MaximizeIcon, MinimizeIcon, RestoreIcon, XIcon } from '@/lib/icons'

const NATIVE_CONTROLS = currentPlatform() === 'macos'

export function WindowControls() {
  const [maximized, setMaximized] = useState(false)

  // Track the real maximized state (double-click on the drag region, WM
  // shortcuts and edge snaps all bypass the toggle button).
  useEffect(() => {
    if (NATIVE_CONTROLS) return
    const win = getCurrentWindow()
    let disposed = false
    let unlisten: (() => void) | undefined
    const sync = () => {
      void win
        .isMaximized()
        .then((m) => {
          if (!disposed) setMaximized(m)
        })
        .catch(() => undefined)
    }
    sync()
    void win
      .onResized(sync)
      .then((fn) => {
        if (disposed) fn()
        else unlisten = fn
      })
      .catch(() => undefined)
    return () => {
      disposed = true
      unlisten?.()
    }
  }, [])

  if (NATIVE_CONTROLS) return null

  const win = getCurrentWindow()
  return (
    <div className="kb-window-controls">
      <button
        type="button"
        className="kb-circle-btn"
        aria-label="Minimize window"
        title="Minimize"
        onClick={() => void win.minimize().catch(() => undefined)}
      >
        <MinimizeIcon />
      </button>
      <button
        type="button"
        className="kb-circle-btn"
        aria-label={maximized ? 'Restore window' : 'Maximize window'}
        title={maximized ? 'Restore' : 'Maximize'}
        onClick={() => void win.toggleMaximize().catch(() => undefined)}
      >
        {maximized ? <RestoreIcon /> : <MaximizeIcon />}
      </button>
      <button
        type="button"
        className="kb-circle-btn is-close"
        aria-label="Close window"
        title="Close"
        onClick={() => void win.close().catch(() => undefined)}
      >
        <XIcon />
      </button>
    </div>
  )
}
