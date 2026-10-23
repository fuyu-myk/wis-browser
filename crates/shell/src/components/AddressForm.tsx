import { useMemo } from 'react'

import type { Address } from '@/bindings/types'
import { countryOptions } from '@/lib/addresses'

interface AddressFormProps {
  value: Address
  busy: boolean
  onChange: (value: Address) => void
  onSave: () => void
  onCancel: () => void
}

interface FieldProps {
  label: string
  value: string
  onChange: (value: string) => void
  autoComplete: string
  type?: string
  wide?: boolean
}

function Field({ label, value, onChange, autoComplete, type = 'text', wide }: FieldProps) {
  return (
    <label className={wide ? 'kb-address-field is-wide' : 'kb-address-field'}>
      <span>{label}</span>
      <input
        type={type}
        className="kb-text-input"
        value={value}
        autoComplete={autoComplete}
        spellCheck={false}
        onChange={(e) => onChange(e.currentTarget.value)}
      />
    </label>
  )
}

export function AddressForm({ value, busy, onChange, onSave, onCancel }: AddressFormProps) {
  const countries = useMemo(() => countryOptions(navigator.language), [])
  const set = (field: keyof Address) => (text: string) => onChange({ ...value, [field]: text })

  return (
    <form
      className="kb-address-form"
      onSubmit={(e) => {
        e.preventDefault()
        onSave()
      }}
    >
      <Field label="First name" value={value.given_name} onChange={set('given_name')} autoComplete="off" />
      <Field label="Last name" value={value.family_name} onChange={set('family_name')} autoComplete="off" />
      <Field
        label="Organization"
        value={value.organization}
        onChange={set('organization')}
        autoComplete="off"
        wide
      />
      <label className="kb-address-field is-wide">
        <span>Street address</span>
        <textarea
          className="kb-text-input"
          rows={2}
          value={value.street_address}
          spellCheck={false}
          onChange={(e) => onChange({ ...value, street_address: e.currentTarget.value })}
        />
      </label>
      <Field label="City" value={value.city} onChange={set('city')} autoComplete="off" />
      <Field label="State, province or region" value={value.region} onChange={set('region')} autoComplete="off" />
      <Field label="Postal code" value={value.postal_code} onChange={set('postal_code')} autoComplete="off" />
      <label className="kb-address-field">
        <span>Country</span>
        <span className="kb-select">
          <select value={value.country} onChange={(e) => onChange({ ...value, country: e.currentTarget.value })}>
            <option value="">None</option>
            {countries.map((country) => (
              <option key={country.code} value={country.code}>
                {country.name}
              </option>
            ))}
          </select>
        </span>
      </label>
      <Field label="Phone" value={value.tel} onChange={set('tel')} autoComplete="off" type="tel" />
      <Field label="Email" value={value.email} onChange={set('email')} autoComplete="off" type="email" />
      <div className="kb-address-actions">
        <button type="button" className="kb-btn" onClick={onCancel}>
          Cancel
        </button>
        <button type="submit" className="kb-btn" disabled={busy}>
          Save address
        </button>
      </div>
    </form>
  )
}
