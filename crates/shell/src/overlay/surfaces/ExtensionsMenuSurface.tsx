/**
 * The extensions menu, hosted by `overlay-extmenu`: each extension running in
 * the workspace, run from here with its popup anchored to the puzzle button.
 */

import { useEffect, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { ExtensionAction } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { EXT_MENU_DONE, EXT_MENU_OPEN, EXT_MENU_READY } from '@/lib/extensionsMenu'
import { menusGlass } from '@/lib/glass'
import { SettingsIcon } from '@/lib/icons'
import { SETTINGS_URL } from '@/lib/url'
import { PuzzleIcon } from '@/components/settingsIcons'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import type { ExtensionsMenuPayload } from '@/hooks/useExtensionsMenu'
import { useThemeApply } from '@/hooks/useThemeApply'

export function ExtensionsMenuSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)
  const [menu, setMenu] = useState<ExtensionsMenuPayload | null>(null)

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<ExtensionsMenuPayload>(EXT_MENU_OPEN, (e) => setMenu(e.payload)).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    void emit(EXT_MENU_READY)
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  if (!menu) return null

  const close = () => {
    setMenu(null)
    void commands.hideOverlay('extmenu').catch(() => undefined)
    void emit(EXT_MENU_DONE)
  }

  const run = (action: ExtensionAction) => {
    const { x, y, width, height } = menu.anchor
    close()
    void commands.extensionRun(action.id, x, y, width, height).catch(() => undefined)
  }

  const openOptions = (action: ExtensionAction) => {
    close()
    void commands.extensionOpenOptions(action.id).catch(() => undefined)
  }

  const manage = () => {
    close()
    const workspaceId = menu.workspaceId
    if (!workspaceId) return
    const settings = state?.tabs.find((t) => t.url === SETTINGS_URL && t.workspace_id === workspaceId)
    if (settings) void commands.setActiveTab(settings.id).catch(() => undefined)
    else void commands.createTab(workspaceId, SETTINGS_URL).catch(() => undefined)
  }

  return (
    <div className={cx('kb-extmenu', { 'glass-menus': menusGlass(state?.settings) })} role="menu">
      {menu.actions.length === 0 ? (
        <div className="kb-extmenu-empty">No extensions run in this workspace.</div>
      ) : (
        menu.actions.map((action) => (
          <div className="kb-extmenu-row" key={action.id}>
            <button
              type="button"
              role="menuitem"
              className="kb-extmenu-item"
              disabled={!action.enabled}
              title={action.label || action.name}
              onClick={() => run(action)}
            >
              {action.icon ? (
                <img className="kb-extmenu-icon" src={action.icon} alt="" />
              ) : (
                <span className="kb-extmenu-icon">
                  <PuzzleIcon />
                </span>
              )}
              <span className="kb-extmenu-name">{action.name}</span>
              {action.badge && <span className="kb-extmenu-badge">{action.badge}</span>}
            </button>
            {action.has_options && (
              <button
                type="button"
                className="kb-extmenu-options"
                title="Options"
                aria-label={`${action.name} options`}
                onClick={() => openOptions(action)}
              >
                <SettingsIcon />
              </button>
            )}
          </div>
        ))
      )}
      <div className="kb-extmenu-divider" />
      <button type="button" role="menuitem" className="kb-extmenu-item" onClick={manage}>
        <span className="kb-extmenu-name">Manage extensions…</span>
      </button>
    </div>
  )
}
