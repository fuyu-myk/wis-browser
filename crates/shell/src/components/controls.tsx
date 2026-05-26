/**
 * Reusable form controls used by the settings popover and the full
 * settings page.
 */

import { cx } from '@/lib/cx'

// ─── Switch ──────────────────────────────────────────────────────────────

interface SwitchProps {
  checked: boolean
  onChange: (next: boolean) => void
  label?: string
}

export function Switch({ checked, onChange, label }: SwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      className={cx('kb-switch', { on: checked })}
      onClick={() => onChange(!checked)}
    />
  )
}

// ─── Segmented ───────────────────────────────────────────────────────────

interface SegmentedOption<T extends string> {
  value: T
  label: string
  icon?: React.ReactNode
}

interface SegmentedProps<T extends string> {
  value: T
  onChange: (next: T) => void
  options: ReadonlyArray<SegmentedOption<T>>
  ariaLabel?: string
}

export function Segmented<T extends string>({
  value,
  onChange,
  options,
  ariaLabel,
}: SegmentedProps<T>) {
  return (
    <div role="radiogroup" aria-label={ariaLabel} className="kb-segmented">
      {options.map((opt) => (
        <button
          key={opt.value}
          type="button"
          role="radio"
          aria-checked={value === opt.value}
          className={cx({ on: value === opt.value })}
          onClick={() => onChange(opt.value)}
        >
          {opt.icon}
          {opt.label}
        </button>
      ))}
    </div>
  )
}

// ─── Swatch row ──────────────────────────────────────────────────────────

interface SwatchOption<T extends string> {
  value: T
  /** CSS class providing a background gradient (e.g. `fav-mint`). */
  swatchClass: string
  label: string
}

interface SwatchRowProps<T extends string> {
  value: T
  onChange: (next: T) => void
  options: ReadonlyArray<SwatchOption<T>>
  ariaLabel?: string
}

export function SwatchRow<T extends string>({
  value,
  onChange,
  options,
  ariaLabel,
}: SwatchRowProps<T>) {
  return (
    <div role="radiogroup" aria-label={ariaLabel} className="kb-swatches">
      {options.map((opt) => (
        <button
          key={opt.value}
          type="button"
          role="radio"
          aria-checked={value === opt.value}
          aria-label={opt.label}
          title={opt.label}
          className={cx('kb-swatch', opt.swatchClass, { on: value === opt.value })}
        onClick={() => onChange(opt.value)}
        />
      ))}
    </div>
  )
}
