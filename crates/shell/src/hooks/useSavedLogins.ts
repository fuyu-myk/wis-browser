/**
 * Chrome-side orchestration for the saved-logins card.
 *
 * `signup.rs` emits `wis-login-field` when a sign-in field takes focus. When
 * the vault has logins for the page's host, the `overlay-logins` card is
 * placed at the field (`useAnchoredCard`) with their usernames. Listing needs
 * no unlock, since usernames are stored in the clear; Touch ID is asked for
 * only once a login is picked. A pick settles the page until it navigates.
 */

import { useEffect, useRef } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { commands } from '@/bindings/commands'
import type { BrowserState, CredentialSummary, TabId } from '@/bindings/types'
import {
  FIELD_GONE_EVENT,
  FIELD_LEAVE_EVENT,
  useAnchoredCard,
  type FieldRect,
} from '@/hooks/useAnchoredCard'

export const LOGINS_SHOW_EVENT = 'wis-logins-show'
export const LOGINS_READY_EVENT = 'wis-logins-ready'
export const LOGINS_CLOSE_EVENT = 'wis-logins-close'
const LOGIN_FIELD_EVENT = 'wis-login-field'

/** In step with `.kb-logins-*` in window.css. */
const CARD = { width: 280, padding: 6, head: 26, row: 36, rows: 5 }

export interface SavedLogins {
  /** Events reach every window, so the surface keeps to its own. */
  window: string
  tabId: TabId
  host: string
  logins: CredentialSummary[]
}

interface TabPayload {
  tabId: string
}

interface LoginField {
  window: string
  tabId: string
  host: string
  rect: FieldRect
}

function cardSize(count: number) {
  return {
    width: CARD.width,
    height: CARD.padding * 2 + CARD.head + Math.min(count, CARD.rows) * CARD.row,
  }
}

export function useSavedLogins(state: BrowserState, suspended: boolean): void {
  const pickRef = useRef<SavedLogins | null>(null)
  const pickedRef = useRef(new Set<TabId>())
  const card = useAnchoredCard('logins', state, suspended, () => {
    pickRef.current = null
  })

  useEffect(() => {
    let cancelled = false
    // Bumped whenever focus moves on or a card closes, so a lookup that
    // finishes after that opens nothing.
    let focus = 0
    const unlistens: UnlistenFn[] = []
    const track = (pending: Promise<UnlistenFn>) =>
      void pending.then((fn) => (cancelled ? fn() : unlistens.push(fn)))

    track(
      listen<LoginField>(LOGIN_FIELD_EVENT, (event) => {
        const field = event.payload
        if (field.window !== getCurrentWindow().label) return
        const tabId = field.tabId as TabId
        const mine = ++focus
        if (pickedRef.current.has(tabId)) return

        void (async () => {
          const logins = await commands
            .autofillCandidates(field.host)
            .catch(() => [] as CredentialSummary[])
          if (cancelled || mine !== focus || logins.length === 0) return

          const size = cardSize(logins.length)
          const rect = card.placeAt(tabId, field.rect, size)
          if (!rect) return
          pickRef.current = { window: field.window, tabId, host: field.host, logins }
          await card.open(tabId, rect, size)
          void emit(LOGINS_SHOW_EVENT, pickRef.current)
        })()
      }),
    )

    track(
      listen<void>(FIELD_LEAVE_EVENT, () => {
        focus++
      }),
    )

    track(
      listen<TabPayload>(FIELD_GONE_EVENT, (event) => {
        focus++
        pickedRef.current.delete(event.payload.tabId as TabId)
      }),
    )

    track(
      listen<string>(LOGINS_READY_EVENT, (event) => {
        if (event.payload !== getCurrentWindow().label) return
        if (pickRef.current) void emit(LOGINS_SHOW_EVENT, pickRef.current)
      }),
    )

    track(
      listen<string>(LOGINS_CLOSE_EVENT, (event) => {
        if (event.payload !== getCurrentWindow().label) return
        focus++
        const open = pickRef.current
        if (open) pickedRef.current.add(open.tabId)
        card.close(true)
      }),
    )

    return () => {
      cancelled = true
      unlistens.forEach((fn) => fn())
    }
  }, [card])
}
