/**
 * The site panel, hosted by `overlay-site`: the active tab's site, its
 * connection and its protection switches.
 */

import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { commands } from '@/bindings/commands'
import type { SiteProtection } from '@/bindings/types'
import { Switch } from '@/components/controls'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import type { SitePanelSize } from '@/features/site/hooks/useSitePanel'
import { useSiteProtection } from '@/features/site/hooks/useSiteProtection'
import { useThemeApply } from '@/hooks/useThemeApply'
import { cx } from '@/lib/cx'
import { menusGlass } from '@/lib/glass'
import { XIcon } from '@/lib/icons'
import { isFeatureUnavailableHere } from '@/lib/platformSupport'
import {
  SITE_PANEL_CLOSE,
  SITE_PANEL_DONE,
  SITE_PANEL_OPEN,
  SITE_PANEL_READY,
  SITE_PANEL_SIZE,
  blockedLine,
  fingerprintingApplies,
  shownHosts,
  siteOf,
  type SitePanelPayload,
} from '@/features/site/lib/sitePanel'
import { forgetSiteRule, siteRules } from '@/features/site/lib/siteRules'
import { connectionKind, SETTINGS_URL } from '@/lib/url'

const CONNECTION = {
  secure: 'Secure connection',
  local: 'Served from this device',
  insecure: 'Not secure',
  wis: '',
}

const TOP_AND_BOTTOM_BORDERS = 2
const BLOCKS_POLL_MS = 1000
const HOSTS_SHOWN = 8

export function SitePanelSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)
  const [panel, setPanel] = useState<SitePanelPayload | null>(null)
  const [expanded, setExpanded] = useState(false)
  const bodyRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    let cancelled = false
    const unlistens: UnlistenFn[] = []
    const keep = (fn: UnlistenFn) => (cancelled ? fn() : unlistens.push(fn))
    const label = getCurrentWindow().label
    void listen<SitePanelPayload>(SITE_PANEL_OPEN, (e) => {
      if (e.payload.window !== label) return
      setExpanded(false)
      setPanel(e.payload)
    }).then(keep)
    void listen<string>(SITE_PANEL_CLOSE, (e) => {
      if (e.payload === label) setPanel(null)
    }).then(keep)
    void emit(SITE_PANEL_READY)
    return () => {
      cancelled = true
      unlistens.forEach((fn) => fn())
    }
  }, [])

  const site = panel ? siteOf(panel.url) : null
  const protection = useSiteProtection(site)
  const counting = !!panel && !isFeatureUnavailableHere('site_block_counts')
  const { data: blocks } = useQuery({
    queryKey: ['page-blocks', panel?.tabId],
    queryFn: () => commands.tabPageBlocks(panel!.tabId),
    enabled: counting,
    refetchInterval: counting ? BLOCKS_POLL_MS : false,
    staleTime: 0,
  })
  const { shown, more } = shownHosts(blocks?.hosts ?? [], HOSTS_SHOWN)
  const counted = !!blocks
  const listed = expanded && counted && blocks.total > 0
  const rules = state && site ? siteRules(state.settings, site) : []
  const workspaceId = state?.tabs.find((t) => t.id === panel?.tabId)?.workspace_id
  const fingerprintingSwitch = fingerprintingApplies(state?.workspaces.find((w) => w.id === workspaceId)?.privacy_mode)

  useLayoutEffect(() => {
    const body = bodyRef.current
    if (!body) return
    const size: SitePanelSize = {
      window: getCurrentWindow().label,
      height: Math.ceil(body.getBoundingClientRect().height) + TOP_AND_BOTTOM_BORDERS,
    }
    void emit(SITE_PANEL_SIZE, size)
  }, [
    panel,
    protection.blocking,
    protection.javascript,
    fingerprintingSwitch,
    counted,
    listed,
    shown.length,
    more,
    rules.length,
  ])

  if (!panel || !site) return null

  const close = () => {
    setPanel(null)
    void commands.hideOverlay('site').catch(() => undefined)
    void emit(SITE_PANEL_DONE)
  }

  const change = (next: SiteProtection) => {
    void commands.tabSetSiteProtection(panel.tabId, site, next).catch(() => undefined)
  }

  const forget = (id: string) => {
    if (!state) return
    void commands.updateSettings(forgetSiteRule(state.settings, site, id)).catch(() => undefined)
  }

  const openSettings = () => {
    close()
    if (!workspaceId) return
    const settings = state?.tabs.find((t) => t.url === SETTINGS_URL && t.workspace_id === workspaceId)
    if (settings) void commands.setActiveTab(settings.id).catch(() => undefined)
    else void commands.createTab(workspaceId, SETTINGS_URL).catch(() => undefined)
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
        {blocks && (
          <div className="kb-sitepanel-blocked">
            <button
              type="button"
              className="kb-sitepanel-link"
              disabled={blocks.total === 0}
              aria-expanded={listed}
              onClick={() => setExpanded(!expanded)}
            >
              {blockedLine(blocks.total)}
            </button>
            {listed && (
              <ul className="kb-sitepanel-hosts">
                {shown.map(([host, count]) => (
                  <li key={host}>
                    <span className="kb-sitepanel-host">{host}</span>
                    <span>{count}</span>
                  </li>
                ))}
                {more > 0 && <li className="kb-sitepanel-note">and {more} more</li>}
              </ul>
            )}
          </div>
        )}
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
        {fingerprintingSwitch && (
          <div className="kb-sitepanel-row">
            <span className="kb-sitepanel-text">Fingerprinting protection</span>
            <Switch
              checked={protection.fingerprinting}
              onChange={(fingerprinting) => change({ ...protection, fingerprinting })}
              label="Fingerprinting protection"
            />
          </div>
        )}
        {rules.length > 0 && (
          <ul className="kb-sitepanel-rules" aria-label={`Saved for ${site}`}>
            {rules.map((rule) => (
              <li key={rule.id}>
                <span className="kb-sitepanel-rule">{rule.label}</span>
                <span className="kb-sitepanel-note">{rule.value}</span>
                <button
                  type="button"
                  className="kb-sitepanel-forget"
                  title="Forget"
                  aria-label={`Forget ${rule.label} for ${site}`}
                  onClick={() => forget(rule.id)}
                >
                  <XIcon />
                </button>
              </li>
            ))}
          </ul>
        )}
        <button type="button" className="kb-sitepanel-link" onClick={openSettings}>
          Open Settings
        </button>
      </div>
    </div>
  )
}
