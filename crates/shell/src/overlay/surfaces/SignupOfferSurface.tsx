/**
 * The generated-password offer, hosted by the `overlay-signup` webview.
 *
 * Remembering a decision promotes it to a standing one: accepted becomes
 * "fill without asking", declined becomes "never offer". Both are undone in
 * Settings, and the standing fill only ever writes an empty field.
 */

import { useEffect, useState } from 'react'
import { emit, listen } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import { Switch } from '@/components/controls'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useThemeApply } from '@/hooks/useThemeApply'
import {
  SIGNUP_CLOSE_EVENT,
  SIGNUP_READY_EVENT,
  SIGNUP_SHOW_EVENT,
  type SignupOffer,
} from '@/hooks/useSignupOffer'
import { cx } from '@/lib/cx'
import { menusGlass } from '@/lib/glass'

export function SignupOfferSurface() {
  const { data: state } = useBrowserState()
  useThemeApply(state?.settings)

  const [offer, setOffer] = useState<SignupOffer | null>(null)
  const [remember, setRemember] = useState(false)

  useEffect(() => {
    let unlisten: (() => void) | null = null
    let cancelled = false
    void listen<SignupOffer>(SIGNUP_SHOW_EVENT, (event) => {
      setOffer(event.payload)
      setRemember(false)
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    void emit(SIGNUP_READY_EVENT)
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  const decide = (accepted: boolean) => {
    const current = offer
    setOffer(null)
    void emit(SIGNUP_CLOSE_EVENT)
    if (!current) return

    if (accepted) {
      void commands.fillGeneratedPassword(current.tabId, current.password).catch(() => undefined)
    }
    if (remember) {
      void commands
        .updateSettings({ password_offer: accepted ? 'always' : 'never' })
        .catch(() => undefined)
    }
  }

  if (!offer) return null

  return (
    <div className={cx('kb-signup-card', { 'glass-menus': menusGlass(state?.settings) })}>
      <div className="kb-signup-head">Use a strong password?</div>
      <div className="kb-signup-host">{offer.host}</div>
      <code className="kb-signup-password">{offer.password}</code>
      <div className="kb-signup-remember">
        <Switch checked={remember} onChange={setRemember} label="Remember this decision" />
        <span>Remember this decision</span>
      </div>
      <div className="kb-signup-actions">
        <button type="button" className="kb-btn" onClick={() => decide(false)}>
          Not now
        </button>
        <button type="button" className="kb-btn is-primary" onClick={() => decide(true)}>
          Use password
        </button>
      </div>
    </div>
  )
}
