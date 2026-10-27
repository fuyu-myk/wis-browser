/**
 * The generated-password offer, hosted by the `overlay-signup` webview.
 *
 * Remembering a decision promotes it to a standing one: accepted becomes
 * "fill without asking", declined becomes "never offer". Both are undone in
 * Settings, and the standing fill only ever writes an empty field.
 */

import { useEffect, useState } from 'react'
import { emit, listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { commands } from '@/bindings/commands'
import { Switch } from '@/components/controls'
import { useWheelStepsAside } from '@/features/vault/hooks/useAnchoredCard'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useThemeApply } from '@/hooks/useThemeApply'
import {
  SIGNUP_CLOSE_EVENT,
  SIGNUP_READY_EVENT,
  SIGNUP_SHOW_EVENT,
  type SignupOffer,
} from '@/features/vault/hooks/useSignupOffer'
import { cx } from '@/lib/cx'
import { menusGlass } from '@/lib/glass'
import { usePickerKeys } from '@/features/vault/hooks/usePickerKeys'
import { FIELD_PICK_EVENT } from '@/features/vault/hooks/useAnchoredCard'

export function SignupOfferSurface() {
  const { data: state } = useBrowserState()
  useThemeApply(state?.settings)
  useWheelStepsAside('signup')

  const [offer, setOffer] = useState<SignupOffer | null>(null)
  const [remember, setRemember] = useState(false)

  useEffect(() => {
    let unlisten: (() => void) | null = null
    let cancelled = false
    const label = getCurrentWindow().label
    void listen<SignupOffer>(SIGNUP_SHOW_EVENT, (event) => {
      if (event.payload.window !== label) return
      setOffer(event.payload)
      setRemember(false)
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    void emit(SIGNUP_READY_EVENT, label)
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  const decide = (accepted: boolean) => {
    const current = offer
    setOffer(null)
    void emit(SIGNUP_CLOSE_EVENT, getCurrentWindow().label)
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

  usePickerKeys(FIELD_PICK_EVENT, offer?.tabId ?? null, () => {
    setOffer(null)
    void emit(SIGNUP_CLOSE_EVENT, getCurrentWindow().label)
  })

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
        <button type="button" className="kb-btn" onClick={() => decide(false)} data-autofocus>
          Not now
        </button>
        <button type="button" className="kb-btn is-primary" onClick={() => decide(true)}>
          Use password
        </button>
      </div>
    </div>
  )
}
