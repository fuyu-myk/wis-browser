/**
 * Chrome-side orchestration for the generated-password offer.
 *
 * `vault/signup.rs` emits `wis-signup-field` when a password field takes focus.
 * With the standing choice set to always, a signup field is filled without a
 * prompt; with it set to never, nothing happens. Otherwise the
 * `overlay-signup` card is placed at the field (`useAnchoredCard`) and handed
 * the password. An explicit answer, either way, settles that page until it
 * navigates, so refocusing the field does not ask again.
 */

import { useEffect, useRef, type RefObject } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { commands } from '@/bindings/commands'
import type { BrowserState, TabId } from '@/bindings/types'
import { FIELD_LEAVE_EVENT, useAnchoredCard, type FieldRect } from '@/features/vault/hooks/useAnchoredCard'
import type { Rect } from '@/features/vault/lib/signupPlacement'

export const SIGNUP_FIELD_EVENT = 'wis-signup-field'
export const SIGNUP_SHOW_EVENT = 'wis-signup-show'
export const SIGNUP_CLOSE_EVENT = 'wis-signup-close'
export const SIGNUP_READY_EVENT = 'wis-signup-ready'
const SIGNUP_GONE_EVENT = 'wis-signup-gone'

const CARD = { width: 340, height: 190 }
/** Used only when the page gave no usable field box. */
const MARGIN = 14

export interface SignupOffer {
  /** Events reach every window, so the surface keeps to its own. */
  window: string
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
  /** It looks like a signup. Other password fields only enable the key button. */
  qualifies: boolean
  rect: FieldRect | null
}

interface TabPayload {
  tabId: string
}

export function useSignupOffer(
  hostRef: RefObject<HTMLElement>,
  state: BrowserState,
  suspended: boolean,
): void {
  const offerRef = useRef<SignupOffer | null>(null)
  /** Pages answered either way, so a refocus does not ask again. */
  const decidedRef = useRef(new Set<TabId>())
  const settingsRef = useRef(state.settings)
  settingsRef.current = state.settings
  const card = useAnchoredCard('signup', state, suspended, () => {
    offerRef.current = null
  })

  useEffect(() => {
    let cancelled = false
    // Bumped whenever focus moves on or a card closes.
    let focus = 0
    const unlistens: UnlistenFn[] = []
    const track = (pending: Promise<UnlistenFn>) =>
      void pending.then((fn) => (cancelled ? fn() : unlistens.push(fn)))

    const fallback = (): Rect | null => {
      const host = hostRef.current?.getBoundingClientRect()
      if (!host) return null
      return {
        x: host.x + Math.max(0, (host.width - CARD.width) / 2),
        y: host.y + MARGIN,
        ...CARD,
      }
    }

    track(
      listen<SignupField>(SIGNUP_FIELD_EVENT, (event) => {
        const field = event.payload
        if (field.window !== getCurrentWindow().label || !field.qualifies) return
        const tabId = field.tabId as TabId
        const offer = settingsRef.current?.password_offer ?? 'ask'
        if (offer === 'never' || decidedRef.current.has(tabId)) return
        const mine = ++focus

        // Already open on this page: follow focus to the field now taking it.
        if (offerRef.current?.tabId === tabId && card.openFor() === tabId) {
          const rect = field.rect ? card.placeAt(tabId, field.rect, CARD) : null
          if (rect) void card.open(tabId, rect, CARD).catch(() => undefined)
          return
        }

        void (async () => {
          const password = await commands
            .generatePasswordForField(field.rules, field.maxLength)
            .catch(() => null)
          if (!password || cancelled || mine !== focus) return

          if (offer === 'always') {
            decidedRef.current.add(tabId)
            void commands.fillGeneratedPassword(tabId, password).catch(() => undefined)
            return
          }

          const rect = field.rect ? card.placeAt(tabId, field.rect, CARD) : fallback()
          if (!rect) return
          offerRef.current = { window: field.window, tabId, host: field.host, password }
          await card.open(tabId, rect, CARD)
          void emit(SIGNUP_SHOW_EVENT, offerRef.current)
        })()
      }),
    )

    track(
      listen<TabPayload>(SIGNUP_GONE_EVENT, (event) => {
        focus++
        decidedRef.current.delete(event.payload.tabId as TabId)
      }),
    )

    track(
      listen<void>(FIELD_LEAVE_EVENT, () => {
        focus++
      }),
    )

    track(
      listen<string>(SIGNUP_READY_EVENT, (event) => {
        if (event.payload !== getCurrentWindow().label) return
        if (offerRef.current) void emit(SIGNUP_SHOW_EVENT, offerRef.current)
      }),
    )

    // The surface only ever closes on an answer, which settles the page.
    track(
      listen<string>(SIGNUP_CLOSE_EVENT, (event) => {
        if (event.payload !== getCurrentWindow().label) return
        focus++
        const open = offerRef.current
        if (open) decidedRef.current.add(open.tabId)
        card.close(true)
      }),
    )

    return () => {
      cancelled = true
      unlistens.forEach((fn) => fn())
    }
  }, [card, hostRef])
}
