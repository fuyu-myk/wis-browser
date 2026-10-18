/**
 * The extensions menu, hosted by `overlay-extmenu`: each extension running in
 * the workspace, run from here with its popup anchored to the puzzle button,
 * or an extension's request for more access.
 */

import { useEffect, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { ExtensionAction } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { EXT_MENU_DONE, EXT_MENU_OPEN, EXT_MENU_READY } from '@/lib/extensionsMenu'
import { menusGlass } from '@/lib/glass'
import { PinIcon, SettingsIcon } from '@/lib/icons'
import { SETTINGS_URL } from '@/lib/url'
import { PuzzleIcon } from '@/components/settingsIcons'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import type { ExtensionsMenuPayload } from '@/hooks/useExtensionsMenu'
import { useThemeApply } from '@/hooks/useThemeApply'
import { ExtensionPrompt } from '@/overlay/surfaces/ExtensionPrompt'

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

  if (menu.kind === 'prompt') {
    return <ExtensionPrompt request={menu.request} glass={menusGlass(state?.settings)} onDone={close} />
  }

  const { anchor, actions, workspaceId } = menu
  const pinned = new Set(state?.settings.extensions.filter((e) => e.pinned).map((e) => e.id))

  const run = (action: ExtensionAction) => {
    const { x, y, width, height } = anchor
    close()
    void commands.extensionRun(action.id, x, y, width, height).catch(() => undefined)
  }

  const openOptions = (action: ExtensionAction) => {
    close()
    void commands.extensionOpenOptions(action.id).catch(() => undefined)
  }

  const manage = () => {
    close()
    if (!workspaceId) return
    const settings = state?.tabs.find((t) => t.url === SETTINGS_URL && t.workspace_id === workspaceId)
    if (settings) void commands.setActiveTab(settings.id).catch(() => undefined)
    else void commands.createTab(workspaceId, SETTINGS_URL).catch(() => undefined)
  }

  return (
    <div className={cx('kb-extmenu', { 'glass-menus': menusGlass(state?.settings) })} role="menu">
      {actions.length === 0 ? (
        <div className="kb-extmenu-empty">No extensions run in this workspace.</div>
      ) : (
        actions.map((action) => (
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
            <button
              type="button"
              className={cx('kb-extmenu-options', { 'is-on': pinned.has(action.id) })}
              title={pinned.has(action.id) ? 'Unpin from the address bar' : 'Pin to the address bar'}
              aria-label={`${pinned.has(action.id) ? 'Unpin' : 'Pin'} ${action.name}`}
              aria-pressed={pinned.has(action.id)}
              onClick={() =>
                void commands.extensionSetPinned(action.id, !pinned.has(action.id)).catch(() => undefined)
              }
            >
              <PinIcon />
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
