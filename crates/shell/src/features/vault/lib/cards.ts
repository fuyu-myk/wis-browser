import type { Card, CardBrand, SavedCard } from '@/bindings/types'
import type { VaultField } from '@/features/vault/lib/vaultSections'

const BRAND_LABELS: Record<CardBrand, string> = {
  visa: 'Visa',
  mastercard: 'Mastercard',
  amex: 'American Express',
  discover: 'Discover',
  jcb: 'JCB',
  diners: 'Diners Club',
  unionpay: 'UnionPay',
  other: 'Card',
}

const YEARS_AHEAD = 15

export interface CardSummaryText {
  title: string
  detail: string
  expired: boolean
}

const twoDigits = (n: number): string => String(n % 100).padStart(2, '0')

/** A saved card's title (its nickname, or network and last digits) and detail line. */
export function cardSummary(card: SavedCard, now: Date): CardSummaryText {
  const masked = `${BRAND_LABELS[card.brand]} •••• ${card.last4}`
  const expired = card.exp_year * 12 + card.exp_month < now.getFullYear() * 12 + now.getMonth() + 1
  const expiry = `${expired ? 'Expired' : 'Expires'} ${twoDigits(card.exp_month)}/${twoDigits(card.exp_year)}`
  if (!card.nickname) return { title: masked, detail: expiry, expired }
  return { title: card.nickname, detail: `${masked} · ${expiry}`, expired }
}

/** The fields an open card row shows: only what is kept unsealed. */
export function cardFields(card: SavedCard, now: Date): VaultField[] {
  const { expired } = cardSummary(card, now)
  const fields: VaultField[] = [
    { label: 'Network', value: BRAND_LABELS[card.brand] },
    { label: 'Number', value: `•••• ${card.last4}` },
    { label: expired ? 'Expired' : 'Expires', value: `${twoDigits(card.exp_month)}/${twoDigits(card.exp_year)}` },
  ]
  return card.nickname ? [...fields, { label: 'Nickname', value: card.nickname }] : fields
}

export function emptyCard(now: Date): Card {
  return { number: '', name: '', exp_month: now.getMonth() + 1, exp_year: now.getFullYear(), nickname: '' }
}

/** The years an expiry can pick: this one onwards, and `current` when it has passed. */
export function expiryYears(now: Date, current: number): number[] {
  const first = now.getFullYear()
  const ahead = Array.from({ length: YEARS_AHEAD + 1 }, (_, i) => first + i)
  return current < first ? [current, ...ahead] : ahead
}
