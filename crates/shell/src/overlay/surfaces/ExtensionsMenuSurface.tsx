/**
 * The extensions menu, hosted by `overlay-extmenu`: each extension running in
 * the workspace, run from here with its popup anchored to the puzzle button,
 * or an extension's request for more access.
 */

import { Fragment, useEffect, useState } from 'react'
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

interface SiteLineProps {
  site: { host: string; allowed: boolean }
  /** Whether stopping it here is up to the user. */
  removable: boolean
  onChange: (allowed: boolean) => void
}

function SiteLine({ site, removable, onChange }: SiteLineProps) {
  return (
    <div className="kb-extmenu-site">
      <span className="kb-extmenu-site-text">
        {site.allowed ? 'Runs on' : 'Wants'} {site.host}
      </span>
      {(!site.allowed || removable) && (
        <button type="button" className="kb-extmenu-site-btn" onClick={() => onChange(!site.allowed)}>
          {site.allowed ? 'Remove' : 'Allow'}
        </button>
      )}
    </div>
  )
}

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

  const workspace = state?.workspaces.find((w) => w.id === workspaceId)
  const listed = (action: ExtensionAction, host: string) =>
    !!state?.settings.extensions.find((e) => e.id === action.id)?.allowed_sites.includes(host)

  const setSite = (action: ExtensionAction, host: string, allowed: boolean) => {
    close()
    const tabId = workspace?.active_tab_id
    void commands
      .extensionSetSiteHere(action.id, host, allowed)
      .then(() => (tabId ? commands.tabReload(tabId) : undefined))
      .catch(() => undefined)
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
          <Fragment key={action.id}>
            <div className="kb-extmenu-row">
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
            {action.site && (
              <SiteLine
                site={action.site}
                removable={!!workspace?.ephemeral || listed(action, action.site.host)}
                onChange={(allowed) => action.site && setSite(action, action.site.host, allowed)}
              />
            )}
          </Fragment>
        ))
      )}
      <div className="kb-extmenu-divider" />
      <button type="button" role="menuitem" className="kb-extmenu-item" onClick={manage}>
        <span className="kb-extmenu-name">Manage extensions…</span>
      </button>
    </div>
  )
}
