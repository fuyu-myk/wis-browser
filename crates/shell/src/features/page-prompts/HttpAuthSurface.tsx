/**
 * Sign-in prompt hosted by the `overlay-httpauth` webview: a site that asked
 * for a username and password. The answer goes to `http_auth_respond`, which
 * hands it to WebKit for the rest of the session; `wis-httpauth-close` tells
 * the chrome to hide the overlay and show any queued prompt.
 */

import { useEffect, useRef, useState, type FormEvent } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { commands } from '@/bindings/commands'
import { cx } from '@/lib/cx'
import { errorMessage } from '@/lib/errors'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'
import { useEscapeKey, useSurfaceFocus } from '@/hooks/useSurfaceFocus'
import { menusGlass } from '@/lib/glass'
import {
  CLOSE_EVENT,
  DISMISS_EVENT,
  READY_EVENT,
  SHOW_EVENT,
  type HttpAuthRequest,
} from '@/features/page-prompts/useHttpAuthOverlay'

const EXIT_MS = 160

export function HttpAuthSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)
  const glassMenus = menusGlass(state?.settings)

  const [req, setReq] = useState<HttpAuthRequest | null>(null)
  const [open, setOpen] = useState(false)
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  // The request on screen, and whether it has been answered: one answer each.
  const shownRef = useRef<number | null>(null)
  const answeredRef = useRef(false)

  useEffect(() => {
    const own = getCurrentWindow().label
    let cancelled = false
    const unlistens: UnlistenFn[] = []
    const keep = (fn: UnlistenFn) => (cancelled ? fn() : unlistens.push(fn))
    const reset = () => {
      setUsername('')
      setPassword('')
      setError(null)
      setBusy(false)
    }

    void listen<HttpAuthRequest>(SHOW_EVENT, (e) => {
      if (e.payload.window !== own) return
      shownRef.current = e.payload.requestId
      answeredRef.current = false
      setReq(e.payload)
      reset()
      requestAnimationFrame(() => setOpen(true))
    }).then(keep)

    void listen<number>(DISMISS_EVENT, (e) => {
      if (shownRef.current !== e.payload) return
      shownRef.current = null
      answeredRef.current = true
      reset()
      setOpen(false)
      setReq(null)
    }).then(keep)

    void emit(READY_EVENT, { window: own })
    return () => {
      cancelled = true
      unlistens.forEach((fn) => fn())
    }
  }, [])

  useSurfaceFocus(open && req?.requestId)
  useEscapeKey(open && !busy, () => cancel())

  if (!req) return null
  const request = req

  function close() {
    setUsername('')
    setPassword('')
    setOpen(false)
    window.setTimeout(() => {
      if (shownRef.current === request.requestId) {
        shownRef.current = null
        setReq(null)
      }
      void emit(CLOSE_EVENT, { window: request.window, requestId: request.requestId })
    }, EXIT_MS)
  }

  function cancel() {
    if (busy || answeredRef.current) return
    answeredRef.current = true
    void commands.httpAuthRespond(request.requestId, null).catch(() => undefined)
    close()
  }

  async function signIn(e: FormEvent) {
    e.preventDefault()
    if (busy || answeredRef.current) return
    setBusy(true)
    setError(null)
    try {
      await commands.httpAuthRespond(request.requestId, { username, password })
      answeredRef.current = true
      close()
    } catch (err: unknown) {
      setError(errorMessage(err))
      setBusy(false)
    }
  }

  return (
    <form
      className={cx('kb-perm-card', 'kb-auth-card', { 'glass-menus': glassMenus, 'is-open': open })}
      role="alertdialog"
      aria-modal="true"
      aria-labelledby="kb-auth-host kb-auth-sub"
      onSubmit={(e) => void signIn(e)}
    >
      <div className="kb-perm-head">
        <span className="kb-perm-glyph" aria-hidden>
          <KeyIcon />
        </span>
        <div className="kb-perm-text">
          <span className="kb-perm-host" id="kb-auth-host">{request.host}</span>
          <span className="kb-perm-sub" id="kb-auth-sub">
            {request.realm ? `asks you to sign in to ${request.realm}` : 'asks you to sign in'}
          </span>
        </div>
      </div>
      {(request.failed || request.insecure || error) && (
        <div className="kb-auth-notes">
          {error && (
            <span className="is-error" role="alert">
              {error}
            </span>
          )}
          {!error && request.failed && (
            <span className="is-error">Incorrect username or password.</span>
          )}
          {request.insecure && (
            <span>This page is not secure, your password will be sent unencrypted.</span>
          )}
        </div>
      )}
      <div className="kb-auth-fields">
        <input
          className="kb-text-input"
          value={username}
          onChange={(e) => setUsername(e.currentTarget.value)}
          placeholder="Username"
          aria-label="Username"
          autoComplete="username"
          spellCheck={false}
          data-autofocus
        />
        <input
          className="kb-text-input"
          type="password"
          value={password}
          onChange={(e) => setPassword(e.currentTarget.value)}
          placeholder="Password"
          aria-label="Password"
          autoComplete="current-password"
        />
      </div>
      <div className="kb-perm-actions">
        <button type="button" className="kb-perm-btn" onClick={cancel} disabled={busy}>
          Cancel
        </button>
        <button type="submit" className="kb-perm-btn is-primary" disabled={busy}>
          Sign in
        </button>
      </div>
    </form>
  )
}

function KeyIcon() {
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
      <circle cx="7.5" cy="15.5" r="4.5" />
      <path d="m10.7 12.3 9.3-9.3" />
      <path d="m16 7 3 3" />
      <path d="m19 4 2 2" />
    </svg>
  )
}
