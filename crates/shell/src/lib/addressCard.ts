/**
 * Event contract for the saved-addresses card at a page's address field
 * (overlay-addresses): the chrome places it and sends what to show; the
 * surface reports picks and asks for a reload after an unlock.
 */

import type { TabId } from '@/bindings/types'

export const ADDRESS_FIELD_EVENT = 'wis-address-field'
export const ADDRESS_MOVE_EVENT = 'wis-address-move'
export const ADDRESS_SCROLL_EVENT = 'wis-address-scroll'
export const ADDRESS_LEAVE_EVENT = 'wis-address-leave'
export const ADDRESSES_SHOW_EVENT = 'wis-addresses-show'
export const ADDRESSES_READY_EVENT = 'wis-addresses-ready'
export const ADDRESSES_CLOSE_EVENT = 'wis-addresses-close'
export const ADDRESSES_RELOAD_EVENT = 'wis-addresses-reload'

export interface AddressRow {
  id: number
  title: string
  detail: string
}

export interface AddressesClose {
  window: string
  tabId: TabId
  picked: boolean
}

export interface AddressOffer {
  window: string
  tabId: TabId
  locked: boolean
  rows: AddressRow[]
  /** The kinds of field a fill would reach, as the page reported them. */
  kinds: string[]
  /** Names the report this offer answers; a fill must match it. */
  token: string
}

/** In step with `.kb-logins-*` and `.kb-addresses-note` in window.css, and the passwords card. */
const CARD = { width: 280, padding: 6, head: 26, note: 30, row: 36, rows: 5 }

export function addressCardSize(rows: number): { width: number; height: number } {
  return {
    width: CARD.width,
    height:
      CARD.padding * 2 + CARD.head + CARD.note + Math.min(Math.max(rows, 1), CARD.rows) * CARD.row,
  }
}

const KIND_LABELS: Record<string, string> = {
  name: 'name',
  given: 'name',
  family: 'name',
  organization: 'organization',
  street: 'street address',
  line1: 'street address',
  line2: 'street address',
  line3: 'street address',
  city: 'city',
  region: 'region',
  postal: 'postal code',
  country: 'country',
  tel: 'phone',
  email: 'email',
}

export function fillsLine(kinds: string[]): string {
  const labels: string[] = []
  for (const kind of kinds) {
    const label = KIND_LABELS[kind]
    if (label && !labels.includes(label)) labels.push(label)
  }
  const listed =
    labels.length > 1 ? `${labels.slice(0, -1).join(', ')} and ${labels[labels.length - 1]}` : labels[0]
  return `Fills ${listed ?? 'nothing'}`
}
