/**
 * Event contract for the saved-cards card at a page's card field
 * (overlay-cards): the chrome places it and sends what to show; the surface
 * reports when it closes. The page's move, scroll and leave reports are the
 * address card's, as one page script reports both.
 */

import type { SavedCard, TabId } from '@/bindings/types'
import { cardSummary } from '@/features/vault/lib/cards'

export const CARD_FIELD_EVENT = 'wis-card-field'
export const CARDS_SHOW_EVENT = 'wis-cards-show'
export const CARDS_READY_EVENT = 'wis-cards-ready'
export const CARDS_CLOSE_EVENT = 'wis-cards-close'

export interface CardRow {
  id: number
  title: string
  detail: string
}

export interface CardOffer {
  window: string
  tabId: TabId
  rows: CardRow[]
  /** The kinds of field a fill would reach, as the page reported them. */
  kinds: string[]
  /** Names the report this offer answers; a fill must match it. */
  token: string
  /** Each fill asks for the vault password rather than Touch ID. */
  password: boolean
}

export interface CardsClose {
  window: string
  tabId: TabId
  picked: boolean
}

const KIND_LABELS: Record<string, string> = {
  number: 'card number',
  name: 'name',
  given: 'name',
  family: 'name',
  exp: 'expiry',
  month: 'expiry',
  year: 'expiry',
  type: 'card type',
}

export function cardFillsLine(kinds: string[]): string {
  const labels: string[] = []
  for (const kind of kinds) {
    const label = KIND_LABELS[kind]
    if (label && !labels.includes(label)) labels.push(label)
  }
  const listed =
    labels.length > 1 ? `${labels.slice(0, -1).join(', ')} and ${labels[labels.length - 1]}` : labels[0]
  return `Fills ${listed ?? 'nothing'}`
}

/** The saved cards worth offering, expired ones left out. */
export function cardRows(saved: SavedCard[], now: Date): CardRow[] {
  return saved.flatMap((card) => {
    const { title, detail, expired } = cardSummary(card, now)
    return expired ? [] : [{ id: card.id, title, detail }]
  })
}
