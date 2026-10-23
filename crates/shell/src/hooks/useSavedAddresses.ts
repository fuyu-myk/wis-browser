/**
 * Chrome-side orchestration for the saved-addresses card.
 *
 * `vault/address_field/mod.rs` emits `wis-address-field` when an address field takes
 * focus. With the setting on and addresses saved, the `overlay-addresses` card
 * is placed at the field: the addresses once the vault is open, else one row
 * to unlock it. A pick settles the page until it navigates.
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
} from '@/hooks/useAnchoredCard'
import {
  ADDRESS_FIELD_EVENT,
  ADDRESS_LEAVE_EVENT,
  ADDRESS_MOVE_EVENT,
  ADDRESS_SCROLL_EVENT,
  ADDRESSES_CLOSE_EVENT,
  ADDRESSES_READY_EVENT,
  ADDRESSES_RELOAD_EVENT,
  ADDRESSES_SHOW_EVENT,
  addressCardSize,
  type AddressesClose,
  type AddressOffer,
} from '@/lib/addressCard'
import { addressSummary } from '@/lib/addresses'

const ADDRESS_CHANNEL: CardChannel = {
  move: ADDRESS_MOVE_EVENT,
  scroll: ADDRESS_SCROLL_EVENT,
  leave: ADDRESS_LEAVE_EVENT,
  release: (tabId) => commands.releaseAddressField(tabId),
}

interface AddressField {
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

async function offerFor(field: AddressField): Promise<AddressOffer | null> {
  const { window, kinds, token } = field
  const tabId = field.tabId as TabId
  const status = await commands.vaultStatus().catch(() => null)
  if (!status?.supported || !status.has_master_key) return null
  if (!(await commands.addressAny().catch(() => false))) return null
  if (!status.unlocked) return { window, tabId, locked: true, rows: [], kinds, token }
  const saved = await commands.addressList().catch(() => [])
  if (saved.length === 0) return null
  const rows = saved.map(({ id, address }) => ({ id, ...addressSummary(address, navigator.language) }))
  return { window, tabId, locked: false, rows, kinds, token }
}

export function useSavedAddresses(state: BrowserState, suspended: boolean): void {
  const offerRef = useRef<AddressOffer | null>(null)
  const fieldRef = useRef<AddressField | null>(null)
  const pickedRef = useRef(new Set<TabId>())
  const enabledRef = useRef(state.settings.address_autofill)
  enabledRef.current = state.settings.address_autofill
  const card = useAnchoredCard(
    'addresses',
    state,
    suspended,
    () => {
      offerRef.current = null
    },
    ADDRESS_CHANNEL,
  )

  useEffect(() => {
    let cancelled = false
    let focus = 0
    const unlistens: UnlistenFn[] = []
    const track = (pending: Promise<UnlistenFn>) =>
      void pending.then((fn) => (cancelled ? fn() : unlistens.push(fn)))

    const offer = (field: AddressField) => {
      const tabId = field.tabId as TabId
      const mine = ++focus
      if (!enabledRef.current || pickedRef.current.has(tabId)) return
      fieldRef.current = field
      void (async () => {
        const next = await offerFor(field)
        if (cancelled || mine !== focus || !next) return
        const size = addressCardSize(next.rows.length)
        const rect = card.placeAt(tabId, field.rect, size)
        if (!rect) return
        offerRef.current = next
        await card.open(tabId, rect, size)
        void emit(ADDRESSES_SHOW_EVENT, next)
      })()
    }

    track(
      listen<AddressField>(ADDRESS_FIELD_EVENT, (event) => {
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
      listen<string>(ADDRESSES_READY_EVENT, (event) => {
        if (event.payload !== getCurrentWindow().label) return
        if (offerRef.current) void emit(ADDRESSES_SHOW_EVENT, offerRef.current)
      }),
    )

    track(
      listen<string>(ADDRESSES_RELOAD_EVENT, (event) => {
        if (event.payload !== getCurrentWindow().label) return
        const field = fieldRef.current
        if (field && card.openFor() === field.tabId) offer(field)
      }),
    )

    track(
      listen<AddressesClose>(ADDRESSES_CLOSE_EVENT, (event) => {
        if (event.payload.window !== getCurrentWindow().label) return
        focus++
        fieldRef.current = null
        if (event.payload.picked) pickedRef.current.add(event.payload.tabId)
        card.close(true)
      }),
    )

    return () => {
      cancelled = true
      unlistens.forEach((fn) => fn())
    }
  }, [card])
}
