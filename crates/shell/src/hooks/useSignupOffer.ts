/**
 * Chrome-side orchestration for the generated-password offer.
 *
 * `signup.rs` emits `wis-signup-field` when a password field takes focus.
 * With the standing choice set to always, a signup field is filled without a
 * prompt; with it set to never, nothing happens. Otherwise the
 * `overlay-signup` webview is placed at the field and handed the password.
 *
 * The card stays with its field: it steps aside while the page scrolls and
 * comes back where the field settles. It closes when focus moves to another
 * field, the field settles out of its pane, the page goes, or the tab stops
 * being the active one. An explicit answer, either way, settles that page until it navigates,
 * so refocusing the field does not ask again.
 */

import { useEffect, useRef, type RefObject } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { commands } from '@/bindings/commands'
import type { BrowserState, TabId } from '@/bindings/types'
import { useOverlay } from '@/hooks/useOverlay'
import { fieldInWindow, placeOffer, type Rect } from '@/lib/signupPlacement'

export const SIGNUP_FIELD_EVENT = 'wis-signup-field'
export const SIGNUP_SHOW_EVENT = 'wis-signup-show'
export const SIGNUP_CLOSE_EVENT = 'wis-signup-close'
export const SIGNUP_READY_EVENT = 'wis-signup-ready'
export const SIGNUP_WHEEL_EVENT = 'wis-signup-wheel'
const SIGNUP_MOVE_EVENT = 'wis-signup-move'
const SIGNUP_SCROLL_EVENT = 'wis-signup-scroll'
const SIGNUP_LEAVE_EVENT = 'wis-signup-leave'
const SIGNUP_GONE_EVENT = 'wis-signup-gone'

const CARD = { width: 340, height: 190 }
const GAP = 6
/** Used only when the page gave no usable field box. */
const MARGIN = 14
/** A wheel over the card that does not scroll the page puts it back after this. */
const WHEEL_RESTORE_MS = 400

export interface SignupOffer {
  tabId: TabId
  host: string
  password: string
}

/** A field's box in its page's CSS pixels, and that page's viewport width. */
interface FieldRect extends Rect {
  viewWidth: number
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

interface MovePayload extends TabPayload {
  rect: FieldRect
}

function activeTabOf(state: BrowserState): TabId | null {
  return state.workspaces.find((w) => w.id === state.active_workspace_id)?.active_tab_id ?? null
}

export function useSignupOffer(hostRef: RefObject<HTMLElement>, state: BrowserState): void {
  const { show, hide } = useOverlay('signup')
  const offerRef = useRef<SignupOffer | null>(null)
  const generatingRef = useRef<TabId | null>(null)
  /** Pages answered either way, so a refocus does not ask again. */
  const decidedRef = useRef(new Set<TabId>())
  const settingsRef = useRef(state.settings)
  settingsRef.current = state.settings
  const activeTab = activeTabOf(state)

  useEffect(() => {
    let cancelled = false
    let placed: Rect | null = null
    let restore: number | undefined
    const unlistens: UnlistenFn[] = []
    const track = (pending: Promise<UnlistenFn>) =>
      void pending.then((fn) => (cancelled ? fn() : unlistens.push(fn)))

    const paneOf = (tabId: TabId): Rect | null => {
      const el = document.querySelector(`[data-pane-leaf="${CSS.escape(tabId)}"]`)
      return el?.getBoundingClientRect() ?? null
    }

    const fallback = (): Rect | null => {
      const host = hostRef.current?.getBoundingClientRect()
      if (!host) return null
      return {
        x: host.x + Math.max(0, (host.width - CARD.width) / 2),
        y: host.y + MARGIN,
        ...CARD,
      }
    }

    /** Where the card goes for a field, or null once the field is out of view. */
    const placeAt = (tabId: TabId, field: FieldRect): Rect | null => {
      const pane = paneOf(tabId)
      if (!pane) return null
      return placeOffer(fieldInWindow(field, field.viewWidth, pane), pane, CARD, GAP)
    }

    const place = (rect: Rect) => {
      window.clearTimeout(restore)
      placed = rect
      void show(rect).catch(() => undefined)
    }

    const stepAside = () => {
      window.clearTimeout(restore)
      void hide().catch(() => undefined)
    }

    const close = (release: boolean) => {
      const open = offerRef.current
      if (!open) return
      offerRef.current = null
      void hide().catch(() => undefined)
      if (release) void commands.releaseSignupField(open.tabId).catch(() => undefined)
    }

    track(
      listen<SignupField>(SIGNUP_FIELD_EVENT, (event) => {
        const field = event.payload
        if (field.window !== getCurrentWindow().label || !field.qualifies) return
        const tabId = field.tabId as TabId
        const offer = settingsRef.current?.password_offer ?? 'ask'
        if (offer === 'never' || decidedRef.current.has(tabId)) return
        if (generatingRef.current === tabId) return

        // Already open on this page: follow focus to the field now taking it.
        if (offerRef.current?.tabId === tabId) {
          const rect = field.rect ? placeAt(tabId, field.rect) : null
          if (rect) place(rect)
          return
        }

        generatingRef.current = tabId
        void (async () => {
          const password = await commands
            .generatePasswordForField(field.rules, field.maxLength)
            .catch(() => null)
          generatingRef.current = null
          if (!password || cancelled) return

          if (offer === 'always') {
            decidedRef.current.add(tabId)
            void commands.fillGeneratedPassword(tabId, password).catch(() => undefined)
            return
          }

          const rect = field.rect ? placeAt(tabId, field.rect) : fallback()
          if (!rect) return
          offerRef.current = { tabId, host: field.host, password }
          placed = rect
          await show(rect)
          void emit(SIGNUP_SHOW_EVENT, offerRef.current)
        })()
      }),
    )

    track(
      listen<MovePayload>(SIGNUP_MOVE_EVENT, (event) => {
        const tabId = event.payload.tabId as TabId
        if (offerRef.current?.tabId !== tabId) return
        const rect = placeAt(tabId, event.payload.rect)
        if (rect) place(rect)
        else close(true)
      }),
    )

    track(
      listen<TabPayload>(SIGNUP_SCROLL_EVENT, (event) => {
        if (offerRef.current?.tabId === event.payload.tabId) stepAside()
      }),
    )

    // Card steps aside to hand the rest of the gesture to the page.
    track(
      listen<void>(SIGNUP_WHEEL_EVENT, () => {
        if (!offerRef.current) return
        stepAside()
        restore = window.setTimeout(() => {
          if (offerRef.current && placed) void show(placed).catch(() => undefined)
        }, WHEEL_RESTORE_MS)
      }),
    )

    track(
      listen<TabPayload>(SIGNUP_LEAVE_EVENT, (event) => {
        if (offerRef.current?.tabId === event.payload.tabId) close(false)
      }),
    )

    track(
      listen<TabPayload>(SIGNUP_GONE_EVENT, (event) => {
        const tabId = event.payload.tabId as TabId
        decidedRef.current.delete(tabId)
        if (offerRef.current?.tabId === tabId) close(false)
      }),
    )

    track(
      listen<void>(SIGNUP_READY_EVENT, () => {
        if (offerRef.current) void emit(SIGNUP_SHOW_EVENT, offerRef.current)
      }),
    )

    // The surface only ever closes on an answer, which settles the page.
    track(
      listen<void>(SIGNUP_CLOSE_EVENT, () => {
        const open = offerRef.current
        if (open) decidedRef.current.add(open.tabId)
        close(true)
      }),
    )

    return () => {
      cancelled = true
      window.clearTimeout(restore)
      unlistens.forEach((fn) => fn())
    }
  }, [show, hide, hostRef])

  useEffect(() => {
    const open = offerRef.current
    if (!open || open.tabId === activeTab) return
    offerRef.current = null
    void hide().catch(() => undefined)
    void commands.releaseSignupField(open.tabId).catch(() => undefined)
  }, [activeTab, hide])
}
