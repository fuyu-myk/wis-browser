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
import type { CredentialSummary, SignupFieldInfo, TabId } from '@/bindings/types'
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
import { errorMessage } from '@/lib/errors'
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

interface PendingUnlock {
  promise: Promise<boolean>
  resolve: (ok: boolean) => void
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
  const [unlocking, setUnlocking] = useState(false)
  const unlockRef = useRef<PendingUnlock | null>(null)
  const refocusRef = useRef<TabId | null>(null)
  const fillRef = useRef<(credentialId: number, forTab: TabId | null) => Promise<void>>(
    async () => undefined,
  )

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
      if (e.payload.fill) void fillRef.current(e.payload.fill.credentialId, e.payload.fill.tab)
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
    const asking = unlockRef.current
    unlockRef.current = null
    setUnlocking(false)
    asking?.resolve(false)
    setAnchor(null)
    void commands.hideOverlay('vaultmenu').catch(() => undefined)
    void emit(VAULTMENU_CLOSE_EVENT).catch(() => undefined)
    const page = refocusRef.current
    refocusRef.current = null
    if (page) void commands.focusWebview(`tab-${page}`).catch(() => undefined)
  }, [])

  const ensureUnlocked = async (forTab: TabId): Promise<boolean> => {
    const status = await commands.vaultStatus().catch(() => null)
    const waiting = unlockRef.current
    if (waiting) return waiting.promise
    if (!status || status.unlocked) return true
    if (!status.password) {
      return commands.vaultUnlock().catch((error: unknown) => {
        void showRemoteToast(getCurrentWindow().label, unlockProblem(error))
        return false
      })
    }
    let resolve: (ok: boolean) => void = () => undefined
    const promise = new Promise<boolean>((done) => {
      resolve = done
    })
    unlockRef.current = { promise, resolve }
    refocusRef.current = forTab
    await commands.focusSelf().catch(() => undefined)
    if (unlockRef.current?.promise === promise) setUnlocking(true)
    return promise
  }

  const finishUnlock = (ok: boolean) => {
    if (!ok) {
      close()
      return
    }
    const asking = unlockRef.current
    unlockRef.current = null
    setUnlocking(false)
    asking?.resolve(true)
  }

  const fill = async (credentialId: number, forTab: TabId | null = tabId) => {
    if (!forTab) return
    setBusy(true)
    try {
      if (!(await ensureUnlocked(forTab))) return
      await commands.autofillFill(forTab, credentialId).catch(() => undefined)
      refocusRef.current = forTab
      close()
    } finally {
      setBusy(false)
    }
  }
  fillRef.current = fill

  const onSave = async () => {
    setBusy(true)
    try {
      if (!tabId || !(await ensureUnlocked(tabId))) return
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

  const hasContent =
    !!unlocking || !!pending || (!!host && candidates.length > 0) || !!passwordField

  return (
    <div className="kb-vaultmenu-backdrop" onMouseDown={close}>
      {hasContent && (
        <div
          className={cx('kb-vaultmenu', { 'glass-menus': glassMenus })}
          role={unlocking ? 'dialog' : 'menu'}
          aria-label={unlocking ? 'Unlock your vault' : undefined}
          style={{ left: anchor.x, top: anchor.y, width: anchor.width }}
          onMouseDown={(e) => e.stopPropagation()}
        >
          {unlocking ? (
            <VaultMenuUnlock onDone={finishUnlock} />
          ) : (
            <>
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
            </>
          )}
        </div>
      )}
    </div>
  )
}

function VaultMenuUnlock({ onDone }: { onDone: (ok: boolean) => void }) {
  const [password, setPassword] = useState('')
  const [error, setError] = useState<unknown>(null)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onDone(false)
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [onDone])

  const submit = async () => {
    if (!password || busy) return
    setBusy(true)
    setError(null)
    try {
      await commands.vaultUnlockPassword(password)
      onDone(true)
    } catch (err) {
      setError(err)
      setPassword('')
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="kb-vaultmenu-save">
      <div className="kb-vaultmenu-save-text">
        <div className="kb-vaultmenu-save-title">Unlock your vault</div>
        <div className="kb-vaultmenu-save-sub">Enter your vault password</div>
      </div>
      <input
        type="password"
        className="kb-vaultmenu-input"
        aria-label="Vault password"
        autoComplete="off"
        autoFocus
        value={password}
        onChange={(e) => setPassword(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') void submit()
        }}
      />
      {error ? (
        <div className="kb-vaultmenu-save-warn" role="alert">
          {errorMessage(error)}
        </div>
      ) : null}
      <div className="kb-vaultmenu-save-actions">
        <button type="button" className="kb-vaultmenu-btn" onClick={() => onDone(false)}>
          Cancel
        </button>
        <button
          type="button"
          className="kb-vaultmenu-btn kb-vaultmenu-btn-primary"
          onClick={() => void submit()}
          disabled={!password || busy}
        >
          Unlock
        </button>
      </div>
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
