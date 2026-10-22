/**
 * URL-bar "vault key" dropdown hosted by the `overlay-vaultmenu` webview.
 *
 * Shows, for the active tab's host:
 *  - a "Save password?" block when a login was just captured;
 *  - the saved-account picker (fill on click; Touch ID on demand);
 *  - a "Manage passwords" row.
 *
 * Mostly self-contained: it reads the active tab from browser state and fetches
 * candidates/pending itself. The chrome key button positions/shows it; this
 * surface reports its measured height back so the overlay is sized to content.
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { commands } from '@/bindings/commands'
import type { CredentialSummary, SignupFieldInfo } from '@/bindings/types'
import { LockIcon } from '@/lib/icons'
import { cx } from '@/lib/cx'
import { SETTINGS_URL } from '@/lib/url'
import {
  CAPTURE_LOGIN_EVENT,
  VAULTMENU_CLOSE_EVENT,
  VAULTMENU_OPEN_EVENT,
  type CaptureInfo,
  type VaultMenuAnchor,
} from '@/lib/vaultMenu'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'
import { menusGlass } from '@/lib/glass'
import { showRemoteToast } from '@/lib/toast'
import { unlockProblem } from '@/lib/vaultRecovery'

function hostOf(url: string): string {
  try {
    return new URL(url).host.replace(/^www\./, '')
  } catch {
    return ''
  }
}

export function VaultMenuSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)
  const glassMenus = menusGlass(state?.settings)

  const activeWorkspace = state?.workspaces.find((w) => w.id === state.active_workspace_id)
  const activeTab =
    activeWorkspace?.active_tab_id != null
      ? state?.tabs.find((t) => t.id === activeWorkspace.active_tab_id) ?? null
      : null
  const host = activeTab ? hostOf(activeTab.url) : ''
  const tabId = activeTab?.id ?? null
  const workspaceId = activeWorkspace?.id ?? null

  const [pending, setPending] = useState<CaptureInfo | null>(null)
  const [candidates, setCandidates] = useState<CredentialSummary[]>([])
  const [passwordField, setPasswordField] = useState<SignupFieldInfo | null>(null)
  const [busy, setBusy] = useState(false)
  const [anchor, setAnchor] = useState<VaultMenuAnchor | null>(null)

  const load = useCallback(async () => {
    const [cands, pend, field] = await Promise.all([
      host
        ? commands.autofillCandidates(host).catch(() => [] as CredentialSummary[])
        : Promise.resolve([] as CredentialSummary[]),
      tabId ? commands.capturePending(tabId).catch(() => null) : Promise.resolve(null),
      tabId ? commands.signupField(tabId).catch(() => null) : Promise.resolve(null),
    ])
    setCandidates(cands)
    setPending(pend)
    setPasswordField(field)
  }, [host, tabId])

  useEffect(() => {
    void load()
  }, [load])

  useEffect(() => {
    let unOpen: UnlistenFn | null = null
    let unCapture: UnlistenFn | null = null
    let cancelled = false
    void listen<VaultMenuAnchor>(VAULTMENU_OPEN_EVENT, (e) => {
      setAnchor(e.payload)
      void load()
    }).then((fn) => (cancelled ? fn() : (unOpen = fn)))
    void listen<CaptureInfo>(CAPTURE_LOGIN_EVENT, () => void load()).then((fn) =>
      cancelled ? fn() : (unCapture = fn),
    )
    return () => {
      cancelled = true
      unOpen?.()
      unCapture?.()
    }
  }, [load])

  const close = useCallback(() => {
    setAnchor(null)
    void commands.hideOverlay('vaultmenu').catch(() => undefined)
    void emit(VAULTMENU_CLOSE_EVENT).catch(() => undefined)
  }, [])

  const ensureUnlocked = async (): Promise<boolean> => {
    const status = await commands.vaultStatus().catch(() => null)
    if (status && !status.unlocked) {
      return commands.vaultUnlock().catch((error: unknown) => {
        void showRemoteToast(getCurrentWindow().label, unlockProblem(error))
        return false
      })
    }
    return true
  }

  const fill = async (credentialId: number) => {
    if (!tabId) return
    setBusy(true)
    try {
      if (!(await ensureUnlocked())) return
      await commands.autofillFill(tabId, credentialId).catch(() => undefined)
      close()
    } finally {
      setBusy(false)
    }
  }

  const onSave = async () => {
    setBusy(true)
    try {
      if (!tabId || !(await ensureUnlocked())) return
      await commands.captureSave(tabId).catch(() => undefined)
      close()
    } finally {
      setBusy(false)
    }
  }

  const onDismiss = () => {
    if (tabId) void commands.captureDismiss(tabId).catch(() => undefined)
    setPending(null)
    if (candidates.length === 0) close()
  }

  // Generated to the field's own rules, the same as the offer would have been.
  const fillStrongPassword = async () => {
    if (!tabId || !passwordField) return
    setBusy(true)
    try {
      const password = await commands.generatePasswordForField(
        passwordField.rules,
        passwordField.maxLength,
      )
      await commands.fillGeneratedPassword(tabId, password)
      close()
    } catch {
      // Nothing was filled, so the menu stays where it is.
    } finally {
      setBusy(false)
    }
  }

  const onManage = () => {
    close()
    if (!workspaceId) return
    const existing = state?.tabs.find(
      (t) => t.url === SETTINGS_URL && t.workspace_id === workspaceId,
    )
    if (existing) {
      if (existing.id !== tabId) void commands.setActiveTab(existing.id).catch(() => undefined)
    } else {
      void commands.createTab(workspaceId, SETTINGS_URL).catch(() => undefined)
    }
  }

  if (!anchor) return null

  const hasContent = !!pending || (!!host && candidates.length > 0) || !!passwordField

  return (
    <div className="kb-vaultmenu-backdrop" onMouseDown={close}>
      {hasContent && (
        <div
          className={cx('kb-vaultmenu', { 'glass-menus': glassMenus })}
          role="menu"
          style={{ left: anchor.x, top: anchor.y, width: anchor.width }}
          onMouseDown={(e) => e.stopPropagation()}
        >
          {pending && (
            <>
          <div className="kb-vaultmenu-save">
            <div className="kb-vaultmenu-save-text">
              <div className="kb-vaultmenu-save-title">
                {pending.replaces ? 'Update saved password?' : 'Save password?'}
              </div>
              <div className="kb-vaultmenu-save-sub">
                {pending.username ? `${pending.username} · ` : ''}
                {pending.host}
              </div>
              {pending.generated && (
                <div className="kb-vaultmenu-save-warn">
                  Wis generated this password and has not saved it anywhere else.
                </div>
              )}
            </div>
            <div className="kb-vaultmenu-save-actions">
              <button type="button" className="kb-vaultmenu-btn" onClick={onDismiss} disabled={busy}>
                Not now
              </button>
              <button
                type="button"
                className="kb-vaultmenu-btn kb-vaultmenu-btn-primary"
                onClick={() => void onSave()}
                disabled={busy}
              >
                {pending.replaces ? 'Update' : 'Save'}
              </button>
            </div>
          </div>
          {candidates.length > 0 && <div className="kb-vaultmenu-sep" />}
        </>
      )}

      {candidates.map((c) => (
        <button
          key={c.id}
          type="button"
          className="kb-vaultmenu-row"
          role="menuitem"
          disabled={busy}
          onClick={() => void fill(c.id)}
        >
          <span className="kb-vaultmenu-ico" aria-hidden>
            <GlobeGlyph />
          </span>
          <span className="kb-vaultmenu-text">
            <span className="kb-vaultmenu-user">{c.username || c.host}</span>
            <span className="kb-vaultmenu-meta">From this website</span>
          </span>
        </button>
      ))}

          {passwordField && (
            <button
              type="button"
              className="kb-vaultmenu-row"
              role="menuitem"
              disabled={busy}
              onClick={() => void fillStrongPassword()}
            >
              <span className="kb-vaultmenu-ico" aria-hidden>
                <LockIcon />
              </span>
              <span className="kb-vaultmenu-text">
                <span className="kb-vaultmenu-user">Use a strong password</span>
                <span className="kb-vaultmenu-meta">Generated to fit this field</span>
              </span>
            </button>
          )}

          {(candidates.length > 0 || passwordField) && <div className="kb-vaultmenu-sep" />}
          <button type="button" className="kb-vaultmenu-manage" role="menuitem" onClick={onManage}>
            Manage passwords
          </button>
        </div>
      )}
    </div>
  )
}

function GlobeGlyph() {
  return (
    <svg
      width={16}
      height={16}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <circle cx="12" cy="12" r="9" />
      <path d="M3 12h18" />
      <path d="M12 3a14 14 0 0 1 0 18 14 14 0 0 1 0-18Z" />
    </svg>
  )
}
