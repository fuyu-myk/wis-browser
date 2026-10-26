/**
 * Chrome-side orchestration for the saved-cards card.
 *
 * `vault/address_field` emits `wis-card-field` when a card field on an https
 * page takes focus. With the setting on and cards saved, the `overlay-cards`
 * card is placed at the field, listing cards by what shows without the vault
 * key; a pick asks Touch ID or the vault password, then fills. A pick settles
 * the page until it navigates.
 */

import { useEffect, useRef } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { commands } from '@/bindings/commands'
import type { BrowserState, TabId } from '@/bindings/types'
import {
  FIELD_GONE_EVENT,
  useAnchoredCard,
  type CardChannel,
  type FieldRect,
  CARD_OPENED_EVENT,
  otherCardOpenFor,
  type CardOpened,
} from '@/features/vault/hooks/useAnchoredCard'
import {
  ADDRESS_LEAVE_EVENT,
  ADDRESS_MOVE_EVENT,
  ADDRESS_SCROLL_EVENT,
  addressCardSize,
} from '@/features/vault/lib/addressCard'
import {
  CARD_FIELD_EVENT,
  CARDS_CLOSE_EVENT,
  CARDS_READY_EVENT,
  CARDS_SHOW_EVENT,
  cardRows,
  type CardOffer,
  type CardsClose,
} from '@/features/vault/lib/cardOffer'

const CARD_CHANNEL: CardChannel = {
  move: ADDRESS_MOVE_EVENT,
  scroll: ADDRESS_SCROLL_EVENT,
  leave: ADDRESS_LEAVE_EVENT,
  release: (tabId) => commands.releaseAddressField(tabId),
}

interface CardField {
  window: string
  tabId: string
  host: string
  rect: FieldRect
  kinds: string[]
  token: string
}

interface TabPayload {
  tabId: string
}

async function offerFor(field: CardField): Promise<CardOffer | null> {
  const { window, kinds, token } = field
  const status = await commands.vaultStatus().catch(() => null)
  if (!status?.supported || !status.has_master_key) return null
  const rows = cardRows(await commands.cardList().catch(() => []), new Date())
  if (rows.length === 0) return null
  return { window, tabId: field.tabId as TabId, rows, kinds, token, password: status.password }
}

export function useSavedCards(state: BrowserState, suspended: boolean): void {
  const offerRef = useRef<CardOffer | null>(null)
  const fieldRef = useRef<CardField | null>(null)
  const pickedRef = useRef(new Set<TabId>())
  const enabledRef = useRef(state.settings.card_autofill)
  enabledRef.current = state.settings.card_autofill
  const card = useAnchoredCard(
    'cards',
    state,
    suspended,
    () => {
      offerRef.current = null
    },
    CARD_CHANNEL,
  )

  useEffect(() => {
    let cancelled = false
    let focus = 0
    const unlistens: UnlistenFn[] = []
    const track = (pending: Promise<UnlistenFn>) =>
      void pending.then((fn) => (cancelled ? fn() : unlistens.push(fn)))

    const offer = (field: CardField) => {
      const tabId = field.tabId as TabId
      const mine = ++focus
      if (!enabledRef.current || pickedRef.current.has(tabId)) return
      fieldRef.current = field
      void (async () => {
        const next = await offerFor(field)
        if (cancelled || mine !== focus || !next) return
        const size = addressCardSize(next.rows.length)
        const rect = card.placeAt(tabId, field.rect, size)
        if (!rect || otherCardOpenFor('cards', tabId)) return
        offerRef.current = next
        await card.open(tabId, rect, size)
        void emit(CARDS_SHOW_EVENT, next)
      })()
    }

    track(
      listen<CardField>(CARD_FIELD_EVENT, (event) => {
        const field = event.payload
        if (field.window !== getCurrentWindow().label) return
        const shown = offerRef.current
        if (shown?.token === field.token && card.openFor() === field.tabId) return
        offer(field)
      }),
    )

    track(
      listen<TabPayload>(ADDRESS_LEAVE_EVENT, (event) => {
        if (fieldRef.current?.tabId !== event.payload.tabId) return
        focus++
        fieldRef.current = null
      }),
    )

    track(
      listen<TabPayload>(FIELD_GONE_EVENT, (event) => {
        pickedRef.current.delete(event.payload.tabId as TabId)
        if (fieldRef.current?.tabId !== event.payload.tabId) return
        focus++
        fieldRef.current = null
      }),
    )

    track(
      listen<string>(CARDS_READY_EVENT, (event) => {
        if (event.payload !== getCurrentWindow().label) return
        if (offerRef.current) void emit(CARDS_SHOW_EVENT, offerRef.current)
      }),
    )

    track(
      listen<CardsClose>(CARDS_CLOSE_EVENT, (event) => {
        if (event.payload.window !== getCurrentWindow().label) return
        focus++
        fieldRef.current = null
        if (event.payload.picked) pickedRef.current.add(event.payload.tabId)
        card.close(true)
      }),
    )

    const yieldToOther = (event: Event) => {
      const { name, tabId } = (event as CustomEvent<CardOpened>).detail
      if (name !== 'cards' && card.openFor() === tabId) card.close(true)
    }
    window.addEventListener(CARD_OPENED_EVENT, yieldToOther)

    return () => {
      cancelled = true
      window.removeEventListener(CARD_OPENED_EVENT, yieldToOther)
      unlistens.forEach((fn) => fn())
    }
  }, [card])
}
