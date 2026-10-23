import { useMemo } from 'react'

import type { Card } from '@/bindings/types'
import { expiryYears } from '@/features/vault/lib/cards'

interface CardFormProps {
  value: Card
  /** The saved card's last digits on an edit. */
  last4: string | null
  busy: boolean
  onChange: (value: Card) => void
  onSave: () => void
  onCancel: () => void
}

const MONTHS = Array.from({ length: 12 }, (_, i) => i + 1)

export function CardForm({ value, last4, busy, onChange, onSave, onCancel }: CardFormProps) {
  const years = useMemo(() => expiryYears(new Date(), value.exp_year), [value.exp_year])
  const set = (field: 'number' | 'name' | 'nickname') => (text: string) => onChange({ ...value, [field]: text })

  return (
    <form
      className="kb-address-form"
      onSubmit={(e) => {
        e.preventDefault()
        onSave()
      }}
    >
      <label className="kb-address-field is-wide">
        <span>Card number</span>
        <input
          type="text"
          inputMode="numeric"
          className="kb-text-input"
          value={value.number}
          placeholder={last4 ? `•••• ${last4}, leave blank to keep` : undefined}
          autoComplete="off"
          spellCheck={false}
          onChange={(e) => set('number')(e.currentTarget.value)}
        />
      </label>
      <label className="kb-address-field is-wide">
        <span>Name on card</span>
        <input
          type="text"
          className="kb-text-input"
          value={value.name}
          autoComplete="off"
          spellCheck={false}
          onChange={(e) => set('name')(e.currentTarget.value)}
        />
      </label>
      <label className="kb-address-field">
        <span>Expiry month</span>
        <span className="kb-select">
          <select
            value={value.exp_month}
            onChange={(e) => onChange({ ...value, exp_month: Number(e.currentTarget.value) })}
          >
            {MONTHS.map((month) => (
              <option key={month} value={month}>
                {String(month).padStart(2, '0')}
              </option>
            ))}
          </select>
        </span>
      </label>
      <label className="kb-address-field">
        <span>Expiry year</span>
        <span className="kb-select">
          <select
            value={value.exp_year}
            onChange={(e) => onChange({ ...value, exp_year: Number(e.currentTarget.value) })}
          >
            {years.map((year) => (
              <option key={year} value={year}>
                {year}
              </option>
            ))}
          </select>
        </span>
      </label>
      <label className="kb-address-field is-wide">
        <span>Nickname</span>
        <input
          type="text"
          className="kb-text-input"
          value={value.nickname}
          placeholder="Optional"
          autoComplete="off"
          spellCheck={false}
          onChange={(e) => set('nickname')(e.currentTarget.value)}
        />
      </label>
      <div className="kb-address-actions">
        <button type="button" className="kb-btn" onClick={onCancel}>
          Cancel
        </button>
        <button type="submit" className="kb-btn" disabled={busy}>
          Save card
        </button>
      </div>
    </form>
  )
}
