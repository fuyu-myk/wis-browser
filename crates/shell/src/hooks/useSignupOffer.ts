/**
 * Chrome-side orchestration for the generated-password offer.
 *
 * `signup.rs` emits `wis-signup-field` when a new-password field takes focus.
 * With the standing choice set to always, the password is filled without a
 * prompt; with it set to never, nothing happens. Otherwise this reveals the
 * `overlay-signup` webview near the top of the content host and hands the
 * surface the password to show.
 *
 * Only one offer per field-focus run: refocusing the same field would
 * otherwise re-prompt on every click.
 */

import { useEffect, useRef, type RefObject } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { commands } from '@/bindings/commands'
import type { TabId, UserSettings } from '@/bindings/types'
import { useOverlay } from '@/hooks/useOverlay'

const FIELD_EVENT = 'wis-signup-field'
export const SIGNUP_SHOW_EVENT = 'wis-signup-show'
export const SIGNUP_CLOSE_EVENT = 'wis-signup-close'
export const SIGNUP_READY_EVENT = 'wis-signup-ready'

const CARD_W = 340
const CARD_H = 190
const MARGIN = 14

export interface SignupOffer {
  tabId: TabId
  host: string
  password: string
}

interface SignupField {
  window: string
  tabId: string
  host: string
  /** The field's `passwordrules`, so the password fits what the site takes. */
  rules: string | null
  maxLength: number | null
}

export function useSignupOffer(
  hostRef: RefObject<HTMLElement>,
  settings: UserSettings | undefined,
): void {
  const overlay = useOverlay('signup')
  const offerRef = useRef<SignupOffer | null>(null)
  const settingsRef = useRef(settings)
  settingsRef.current = settings

  useEffect(() => {
    let cancelled = false
    const unlistens: UnlistenFn[] = []

    const rectFor = () => {
      const el = hostRef.current
      if (!el) return null
      const r = el.getBoundingClientRect()
      return {
        x: r.x + Math.max(0, (r.width - CARD_W) / 2),
        y: r.y + MARGIN,
        width: CARD_W,
        height: CARD_H,
      }
    }

    void listen<SignupField>(FIELD_EVENT, (event) => {
      if (event.payload.window !== getCurrentWindow().label) return
      const offer = settingsRef.current?.password_offer ?? 'ask'
      if (offer === 'never') return
      // One prompt per focus run.
      if (offerRef.current?.tabId === (event.payload.tabId as TabId)) return

      void (async () => {
        const password = await commands
          .generatePasswordForField(event.payload.rules, event.payload.maxLength)
          .catch(() => null)
        if (!password || cancelled) return

        const tabId = event.payload.tabId as TabId
        if (offer === 'always') {
          void commands.fillGeneratedPassword(tabId, password).catch(() => undefined)
          return
        }

        const rect = rectFor()
        if (!rect) return
        offerRef.current = { tabId, host: event.payload.host, password }
        await overlay.show(rect)
        void emit(SIGNUP_SHOW_EVENT, offerRef.current)
      })()
    }).then((fn) => (cancelled ? fn() : unlistens.push(fn)))

    void listen<void>(SIGNUP_READY_EVENT, () => {
      if (offerRef.current) void emit(SIGNUP_SHOW_EVENT, offerRef.current)
    }).then((fn) => (cancelled ? fn() : unlistens.push(fn)))

    void listen<void>(SIGNUP_CLOSE_EVENT, () => {
      offerRef.current = null
      void overlay.hide().catch(() => undefined)
    }).then((fn) => (cancelled ? fn() : unlistens.push(fn)))

    return () => {
      cancelled = true
      unlistens.forEach((fn) => fn())
    }
  }, [overlay, hostRef])
}
