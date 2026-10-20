/**
 * The site panel, hosted by `overlay-site`: the active tab's site, its
 * connection and its protection switches.
 */

import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { commands } from '@/bindings/commands'
import type { SiteProtection } from '@/bindings/types'
import { Switch } from '@/components/controls'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import type { SitePanelSize } from '@/hooks/useSitePanel'
import { useSiteProtection } from '@/hooks/useSiteProtection'
import { useThemeApply } from '@/hooks/useThemeApply'
import { cx } from '@/lib/cx'
import { menusGlass } from '@/lib/glass'
import { isFeatureUnavailableHere } from '@/lib/platformSupport'
import {
  SITE_PANEL_DONE,
  SITE_PANEL_OPEN,
  SITE_PANEL_READY,
  SITE_PANEL_SIZE,
  siteOf,
  type SitePanelPayload,
} from '@/lib/sitePanel'
import { connectionKind } from '@/lib/url'

const CONNECTION = {
  secure: 'Secure connection',
  local: 'Served from this device',
  insecure: 'Not secure',
  wis: '',
}

const TOP_AND_BOTTOM_BORDERS = 2

export function SitePanelSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)
  const [panel, setPanel] = useState<SitePanelPayload | null>(null)
  const bodyRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    const label = getCurrentWindow().label
    void listen<SitePanelPayload>(SITE_PANEL_OPEN, (e) => {
      if (e.payload.window === label) setPanel(e.payload)
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    void emit(SITE_PANEL_READY)
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  const site = panel ? siteOf(panel.url) : null
  const protection = useSiteProtection(site)

  useLayoutEffect(() => {
    const body = bodyRef.current
    if (!body) return
    const size: SitePanelSize = {
      window: getCurrentWindow().label,
      height: Math.ceil(body.getBoundingClientRect().height) + TOP_AND_BOTTOM_BORDERS,
    }
    void emit(SITE_PANEL_SIZE, size)
  }, [panel, protection.blocking, protection.javascript])

  if (!panel || !site) return null

  const close = () => {
    setPanel(null)
    void commands.hideOverlay('site').catch(() => undefined)
    void emit(SITE_PANEL_DONE)
  }

  const change = (next: SiteProtection) => {
    void commands.tabSetSiteProtection(panel.tabId, site, next).catch(() => undefined)
  }

  return (
    <div
      className={cx('kb-extmenu kb-sitepanel', { 'glass-menus': menusGlass(state?.settings) })}
      role="dialog"
      aria-label={`Settings for ${site}`}
      onKeyDown={(e) => {
        if (e.key === 'Escape') close()
      }}
    >
      <div ref={bodyRef} className="kb-sitepanel-body">
        <div className="kb-sitepanel-head">
          <span className="kb-sitepanel-site">{site}</span>
          <span className="kb-sitepanel-note">{CONNECTION[connectionKind(panel.url)]}</span>
        </div>
        <div className="kb-sitepanel-row">
          <span className="kb-sitepanel-text">
            <span>Block trackers and ads</span>
            {!protection.blocking && (
              <span className="kb-sitepanel-note">Known tracker and ad servers stay blocked.</span>
            )}
          </span>
          <Switch
            checked={protection.blocking}
            onChange={(blocking) => change({ ...protection, blocking })}
            label="Block trackers and ads"
          />
        </div>
        {!isFeatureUnavailableHere('site_javascript') && (
          <div className="kb-sitepanel-row">
            <span className="kb-sitepanel-text">JavaScript</span>
            <Switch
              checked={protection.javascript}
              onChange={(javascript) => change({ ...protection, javascript })}
              label="JavaScript"
            />
          </div>
        )}
      </div>
    </div>
  )
}
