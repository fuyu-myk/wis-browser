/**
 * Open-in-app prompt hosted by the `overlay-applaunch` webview. The chrome
 * forwards the cancelled custom-scheme navigation via `wis-applaunch-show`;
 * the user's choice resolves it through `app_launch_decide` (which performs
 * the OS handoff) and `wis-applaunch-close` tells the chrome to hide the
 * overlay (and show any queued request).
 */

import { useEffect, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import { cx } from '@/lib/cx'
import { Switch } from '@/components/controls'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'
import { menusGlass } from '@/lib/glass'
import type { AppLaunchRequest } from '@/hooks/useAppLaunchOverlay'

const SHOW_EVENT = 'wis-applaunch-show'
const CLOSE_EVENT = 'wis-applaunch-close'
const READY_EVENT = 'wis-applaunch-ready'
const EXIT_MS = 160

export function AppLaunchSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)
  const glassMenus = menusGlass(state?.settings)

  const [req, setReq] = useState<AppLaunchRequest | null>(null)
  const [open, setOpen] = useState(false)
  const [remember, setRemember] = useState(false)

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<AppLaunchRequest>(SHOW_EVENT, (e) => {
      setReq(e.payload)
      setRemember(false)
      requestAnimationFrame(() => setOpen(true))
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    void emit(READY_EVENT)
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  if (!req) return null

  const decide = (granted: boolean) => {
    void commands.appLaunchDecide(req.requestId, granted, remember).catch(() => undefined)
    if (remember && req.initiatorHost) {
      const current = state?.settings.app_launch_permissions ?? {}
      const key = `${req.scheme}|${req.initiatorHost}`
      void commands
        .updateSettings({ app_launch_permissions: { ...current, [key]: granted } })
        .catch(() => undefined)
    }
    setOpen(false)
    window.setTimeout(() => {
      setReq(null)
      void emit(CLOSE_EVENT)
    }, EXIT_MS)
  }

  const appLabel = req.appName ?? 'another app'
  const canRemember = req.hasHandler && req.initiatorHost !== null

  return (
    <div
      className={cx('kb-perm-card', { 'glass-menus': glassMenus, 'is-open': open })}
      role="alertdialog"
      aria-label="Open in app"
    >
      <div className="kb-perm-head">
        <span className="kb-perm-glyph" aria-hidden>
          <LaunchIcon />
        </span>
        <div className="kb-perm-text">
          <span className="kb-perm-host">{req.initiatorHost ?? 'This page'}</span>
          <span className="kb-perm-sub">
            {req.hasHandler
              ? `wants to open ${appLabel}`
              : `links to an app that is not installed`}
          </span>
        </div>
      </div>
      <div className="kb-applaunch-url" title={req.url}>
        {req.url}
      </div>
      {canRemember && (
        <div className="kb-perm-remember">
          <Switch checked={remember} onChange={setRemember} label="Remember this decision" />
          <span>Remember this decision</span>
        </div>
      )}
      <div className="kb-perm-actions">
        {req.hasHandler ? (
          <>
            <button type="button" className="kb-perm-btn" onClick={() => decide(false)}>
              Cancel
            </button>
            <button
              type="button"
              className="kb-perm-btn is-primary"
              onClick={() => decide(true)}
              autoFocus
            >
              Open
            </button>
          </>
        ) : (
          <button
            type="button"
            className="kb-perm-btn"
            onClick={() => decide(false)}
            autoFocus
          >
            Close
          </button>
        )}
      </div>
    </div>
  )
}

function LaunchIcon() {
  return (
    <svg
      width={18}
      height={18}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
      <path d="M15 3h6v6" />
      <path d="M10 14 21 3" />
    </svg>
  )
}
