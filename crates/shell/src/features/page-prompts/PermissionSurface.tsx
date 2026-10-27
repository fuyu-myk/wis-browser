/**
 * Camera/microphone consent prompt hosted by the `overlay-permission` webview.
 * The chrome (`usePermissionOverlay`) anchors this webview to the top-right of
 * the page and forwards the request via `wis-permission-show`; the user's
 * choice resolves it through `resolve_media_permission` and `wis-permission-close`
 * tells the chrome to hide the overlay (and show any queued request).
 */

import { useEffect, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import { cx } from '@/lib/cx'
import { Switch } from '@/components/controls'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'
import { useEscapeKey, useSurfaceFocus } from '@/hooks/useSurfaceFocus'
import { menusGlass } from '@/lib/glass'

const SHOW_EVENT = 'wis-permission-show'
const CLOSE_EVENT = 'wis-permission-close'
const READY_EVENT = 'wis-permission-ready'
const EXIT_MS = 160

interface PermissionRequest {
  request_id: number
  origin: string
  kind: string
}

/** Authority (host[:port]) of `scheme://host[:port]/...`. */
function hostOf(origin: string): string {
  const rest = origin.includes('://') ? origin.split('://')[1] : origin
  return rest.split('/')[0] || origin
}

function deviceLabel(kind: string): string {
  if (kind === 'microphone') return 'microphone'
  if (kind === 'camera+microphone') return 'camera and microphone'
  return 'camera'
}

export function PermissionSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)
  const glassMenus = menusGlass(state?.settings)

  const [req, setReq] = useState<PermissionRequest | null>(null)
  const [open, setOpen] = useState(false)
  // Persist the decision across restarts; off makes it last only this session.
  const [remember, setRemember] = useState(true)

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<PermissionRequest>(SHOW_EVENT, (e) => {
      setReq(e.payload)
      setRemember(true)
      requestAnimationFrame(() => setOpen(true))
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    // Replay the current request in case the chrome showed it before mount.
    void emit(READY_EVENT)
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  useSurfaceFocus(open && req?.request_id)
  useEscapeKey(open, () => decide(false, false))

  if (!req) return null

  function decide(granted: boolean, keep = remember) {
    if (!req) return
    void commands.resolveMediaPermission(req.request_id, granted, keep).catch(() => undefined)
    if (keep) {
      const current = state?.settings.media_permissions ?? {}
      const key = `${req.kind}|${req.origin}`
      void commands
        .updateSettings({ media_permissions: { ...current, [key]: granted } })
        .catch(() => undefined)
    }
    setOpen(false)
    window.setTimeout(() => {
      setReq(null)
      void emit(CLOSE_EVENT)
    }, EXIT_MS)
  }

  return (
    <div
      className={cx('kb-perm-card', { 'glass-menus': glassMenus, 'is-open': open })}
      role="alertdialog"
      aria-modal="true"
      aria-label="Permission request"
    >
      <div className="kb-perm-head">
        <span className="kb-perm-glyph" aria-hidden>
          {req.kind === 'microphone' ? <MicIcon /> : <CameraIcon />}
        </span>
        <div className="kb-perm-text">
          <span className="kb-perm-host">{hostOf(req.origin)}</span>
          <span className="kb-perm-sub">wants to use your {deviceLabel(req.kind)}</span>
        </div>
      </div>
      <div className="kb-perm-remember">
        <Switch checked={remember} onChange={setRemember} label="Remember this decision" />
        <span>Remember this decision</span>
      </div>
      <div className="kb-perm-actions">
        <button type="button" className="kb-perm-btn" onClick={() => decide(false)} data-autofocus>
          Block
        </button>
        <button type="button" className="kb-perm-btn is-primary" onClick={() => decide(true)}>
          Allow
        </button>
      </div>
    </div>
  )
}

function CameraIcon() {
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
      <path d="m23 7-7 5 7 5V7Z" />
      <rect x="1" y="5" width="15" height="14" rx="2" />
    </svg>
  )
}

function MicIcon() {
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
      <path d="M12 2a3 3 0 0 0-3 3v6a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
      <path d="M19 10v1a7 7 0 0 1-14 0v-1M12 18v4M8 22h8" />
    </svg>
  )
}
