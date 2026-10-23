/**
 * The extensions menu, hosted by `overlay-extmenu`: each extension running in
 * the workspace, run from here with its popup anchored to the puzzle button,
 * an extension's request for more access, or adding a Chrome Web Store page's
 * extension.
 */

import { Fragment, useEffect, useRef, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { ExtensionAction, StoreOffer } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { EXT_MENU_DONE, EXT_MENU_OPEN, EXT_MENU_READY, EXT_STORE_DECLINED } from '@/features/extensions/lib/extensionsMenu'
import { menusGlass } from '@/lib/glass'
import { PinIcon, SettingsIcon } from '@/lib/icons'
import { SETTINGS_URL } from '@/lib/url'
import { PuzzleIcon } from '@/components/settingsIcons'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import type { ExtensionsMenuPayload } from '@/features/extensions/hooks/useExtensionsMenu'
import { useThemeApply } from '@/hooks/useThemeApply'
import { ExtensionPrompt } from '@/features/extensions/components/ExtensionPrompt'
import { ExtensionStoreAdd } from '@/features/extensions/components/ExtensionStoreAdd'
import { ExtensionStoreLink } from '@/features/extensions/components/ExtensionStoreLink'

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
  const [adding, setAdding] = useState<string | null>(null)
  const [linking, setLinking] = useState<Extract<StoreOffer, { kind: 'link' }> | null>(null)
  const stagedRef = useRef<string | null>(null)

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    const open = (payload: ExtensionsMenuPayload) => {
      if (stagedRef.current) void commands.extensionDiscard(stagedRef.current).catch(() => undefined)
      stagedRef.current = null
      setAdding(null)
      setLinking(null)
      setMenu(payload)
    }
    void listen<ExtensionsMenuPayload>(EXT_MENU_OPEN, (e) => open(e.payload)).then((fn) => {
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
    setAdding(null)
    setLinking(null)
    void commands.hideOverlay('extmenu').catch(() => undefined)
    void emit(EXT_MENU_DONE)
  }

  if (linking) {
    return (
      <ExtensionStoreLink
        id={linking.id}
        storeId={linking.store_id}
        name={linking.name}
        glass={menusGlass(state?.settings)}
        onDone={close}
      />
    )
  }

  if (adding) {
    return (
      <ExtensionStoreAdd
        storeId={adding}
        glass={menusGlass(state?.settings)}
        stagedRef={stagedRef}
        onDone={close}
      />
    )
  }

  if (menu.kind === 'prompt') {
    return <ExtensionPrompt request={menu.request} glass={menusGlass(state?.settings)} onDone={close} />
  }

  const { anchor, actions, workspaceId, offer } = menu

  const decline = (offered: StoreOffer) => {
    if (offered.kind === 'add') void emit(EXT_STORE_DECLINED, offered.store_id)
    else void commands.extensionDeclineListing(offered.id, offered.store_id).catch(() => undefined)
    close()
  }

  const accept = (offered: StoreOffer) => {
    if (offered.kind === 'add') setAdding(offered.store_id)
    else setLinking(offered)
  }
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
      {offer && (
        <>
          <div className="kb-extmenu-offer">
            <span className="kb-extmenu-offer-text">
              {offer.kind === 'add'
                ? 'Add this page’s extension to Wis?'
                : `Get updates for ${offer.name} from this listing?`}
            </span>
            <div className="kb-perm-actions">
              <button type="button" className="kb-perm-btn" onClick={() => decline(offer)}>
                No
              </button>
              <button type="button" className="kb-perm-btn is-primary" onClick={() => accept(offer)}>
                Yes
              </button>
            </div>
          </div>
          <div className="kb-extmenu-divider" />
        </>
      )}
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
