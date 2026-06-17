/**
 * Navigation safety interstitial, hosted by the `overlay-navguard` webview.
 * Shown full-window when a real navigation to a Medium/High-risk page is halted
 * by the native guard. The user must choose "Take me back" (go back) or
 * "Proceed anyway" (load the page, remembering the host for the session).
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { LinkAssessment, TabId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'
import { menusGlass } from '@/lib/glass'

const NAV_GUARD_SHOW_EVENT = 'wis-navguard-show'
const NAV_GUARD_CLOSE_EVENT = 'wis-navguard-close'
const NAV_GUARD_READY_EVENT = 'wis-navguard-ready'

interface NavGuardPayload {
  url: string
  host: string
  assessment: LinkAssessment
  navigated: boolean
  tab_id: string | null
}

const RISK_LABEL: Record<LinkAssessment['risk'], string> = {
  low: 'Caution',
  medium: 'Suspicious site',
  high: 'Dangerous site',
}

export function NavGuardSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)
  const glassMenus = menusGlass(state?.settings)

  const activeWorkspace = state?.workspaces.find((w) => w.id === state.active_workspace_id)
  const activeTabId: TabId | null = activeWorkspace?.active_tab_id ?? null
  const activeTabRef = useRef<TabId | null>(null)
  activeTabRef.current = activeTabId

  const [data, setData] = useState<NavGuardPayload | null>(null)
  const dataRef = useRef<NavGuardPayload | null>(null)
  dataRef.current = data

  const close = useCallback(() => {
    setData(null)
    void emit(NAV_GUARD_CLOSE_EVENT)
    void commands.hideOverlay('navguard').catch(() => undefined)
  }, [])

  useEffect(() => {
    let cancelled = false
    let unlisten: UnlistenFn | null = null
    void listen<NavGuardPayload>(NAV_GUARD_SHOW_EVENT, (e) => {
      setData(e.payload)
    }).then((fn) => {
      if (cancelled) fn()
      else {
        unlisten = fn
        void emit(NAV_GUARD_READY_EVENT)
      }
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  // For an intercepted click nothing navigated, so "Take me back" just dismisses
  // (the user stays put). For the page-load fallback the tab already moved, so
  // step back in history.
  const takeMeBack = useCallback(() => {
    const d = dataRef.current
    if (d?.navigated) {
      const tabId = (d.tab_id ?? activeTabRef.current) as TabId | null
      if (tabId) void commands.tabGoBack(tabId).catch(() => undefined)
    }
    close()
  }, [close])

  const proceed = useCallback(() => {
    const d = dataRef.current
    const tabId = (d?.tab_id ?? activeTabRef.current) as TabId | null
    if (d && tabId) void commands.navGuardProceed(tabId, d.url, d.host).catch(() => undefined)
    close()
  }, [close])

  if (!data) return null

  const risk = data.assessment.risk
  const warnings = data.assessment.warnings

  return (
    <div className={cx('kb-navguard-backdrop', `risk-${risk}`, { 'glass-menus': glassMenus })}>
      <div className={cx('kb-navguard', { 'glass-menus': glassMenus })} role="alertdialog" aria-modal="true">
        <div className="kb-navguard-icon">
          <ShieldGlyph />
        </div>
        <div className={cx('kb-navguard-level', `risk-${risk}`)}>{RISK_LABEL[risk]}</div>
        <h1 className="kb-navguard-title">Wis stopped this page before it loaded</h1>
        <p className="kb-navguard-host">{data.host}</p>
        <ul className="kb-navguard-reasons">
          {warnings.map((w) => (
            <li key={w.code}>{w.message}</li>
          ))}
        </ul>
        <div className="kb-navguard-actions">
          <button type="button" className="kb-navguard-back" onClick={takeMeBack}>
            Take me back
          </button>
          <button type="button" className="kb-navguard-proceed" onClick={proceed}>
            Proceed anyway
          </button>
        </div>
        <p className="kb-navguard-foot">
          Only continue if you trust this site. Proceeding loads it normally for the rest of
          this session.
        </p>
      </div>
    </div>
  )
}

function ShieldGlyph() {
  return (
    <svg
      width={28}
      height={28}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10Z" />
      <line x1="12" y1="9" x2="12" y2="13" />
      <line x1="12" y1="16" x2="12.01" y2="16" />
    </svg>
  )
}
