/**
 * Reusable form controls used by the settings popover and the full
 * settings page.
 */

import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from 'react'

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
  /** Shown but locked (e.g. a platform pins one value). */
  disabled?: boolean
}

export function Segmented<T extends string>({
  value,
  onChange,
  options,
  ariaLabel,
  disabled = false,
}: SegmentedProps<T>) {
  const rootRef = useRef<HTMLDivElement | null>(null)
  const [thumb, setThumb] = useState<{ left: number; width: number } | null>(null)

  // The sliding thumb tracks the checked option's box; measured because
  // option widths are content-driven.
  const measure = useCallback(() => {
    const btn = rootRef.current?.querySelector<HTMLButtonElement>(
      'button[aria-checked="true"]',
    )
    if (!btn) return
    const next = { left: btn.offsetLeft, width: btn.offsetWidth }
    setThumb((prev) =>
      prev && prev.left === next.left && prev.width === next.width ? prev : next,
    )
  }, [])
  useLayoutEffect(measure, [value, options, measure])
  // Button widths shift without a value change (font load, zoom).
  useEffect(() => {
    const root = rootRef.current
    if (!root) return
    const ro = new ResizeObserver(measure)
    ro.observe(root)
    return () => ro.disconnect()
  }, [measure])

  return (
    <div
      role="radiogroup"
      aria-label={ariaLabel}
      aria-disabled={disabled || undefined}
      className={`kb-segmented${disabled ? ' is-disabled' : ''}`}
      ref={rootRef}
    >
      {thumb && (
        <span
          className="kb-segmented-thumb"
          style={{ transform: `translateX(${thumb.left}px)`, width: thumb.width }}
          aria-hidden
        />
      )}
      {options.map((opt) => (
        <button
          key={opt.value}
          type="button"
          role="radio"
          aria-checked={value === opt.value}
          disabled={disabled}
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

// ─── ColorPicker ─────────────────────────────────────────────────────────
// Hand-rolled (no library): a swatch button opening a popover with a
// saturation/value square, a hue slider, and a hex field. HSV<->hex
// conversions are pure helpers below.

interface ColorPickerProps {
  /** `#rrggbb` hex colour. */
  value: string
  onChange: (hex: string) => void
  label?: string
  /** Open the popover on mount (tentative-swatch flows). */
  autoOpen?: boolean
  /** Renders a confirm (+) button in the popover; closing without it cancels. */
  onConfirm?: () => void
  /** Called when the popover closes without confirming (Esc / click away). */
  onCancel?: () => void
}

export function ColorPicker({
  value,
  onChange,
  label,
  autoOpen = false,
  onConfirm,
  onCancel,
}: ColorPickerProps) {
  const [open, setOpen] = useState(autoOpen)
  const [hexDraft, setHexDraft] = useState(value)
  const rootRef = useRef<HTMLDivElement | null>(null)
  const norm = normalizeHex(value) ?? '#000000'
  const alpha = norm.length === 9 ? parseInt(norm.slice(7, 9), 16) / 255 : 1
  const hsv = hexToHsv(norm.slice(0, 7)) ?? { h: 0, s: 0, v: 1 }

  const emit = (nextHsv: Hsv, nextAlpha: number = alpha) => {
    const byte = Math.round(nextAlpha * 255)
    const suffix = byte >= 255 ? '' : byte.toString(16).padStart(2, '0')
    onChange(hsvToHex(nextHsv) + suffix)
  }

  useEffect(() => setHexDraft(value), [value])

  useEffect(() => {
    if (!open) return
    const dismiss = () => {
      setOpen(false)
      onCancel?.()
    }
    const onDown = (e: PointerEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) dismiss()
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') dismiss()
    }
    document.addEventListener('pointerdown', onDown)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('pointerdown', onDown)
      document.removeEventListener('keydown', onKey)
    }
  }, [open])

  const pickSv = (e: ReactPointerEvent<HTMLDivElement>) => {
    const rect = e.currentTarget.getBoundingClientRect()
    const s = Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width))
    const v = 1 - Math.min(1, Math.max(0, (e.clientY - rect.top) / rect.height))
    emit({ ...hsv, s, v })
  }

  const submitHex = () => {
    const norm = normalizeHex(hexDraft)
    if (norm) onChange(norm)
    else setHexDraft(value)
  }

  return (
    <div className="kb-colorpick" ref={rootRef}>
      <button
        type="button"
        className="kb-colorpick-swatch"
        style={{ background: value }}
        aria-label={label ?? 'Pick colour'}
        title={label ?? value}
        onClick={() => setOpen((v) => !v)}
      />
      {open && (
        <div className="kb-colorpick-pop" role="dialog" aria-label="Colour picker">
          <div
            className="kb-colorpick-sv"
            style={{ backgroundColor: hsvToHex({ h: hsv.h, s: 1, v: 1 }) }}
            onPointerDown={(e) => {
              e.currentTarget.setPointerCapture(e.pointerId)
              pickSv(e)
            }}
            onPointerMove={(e) => {
              if (e.buttons & 1) pickSv(e)
            }}
            role="presentation"
          >
            <span
              className="kb-colorpick-dot"
              style={{ left: `${hsv.s * 100}%`, top: `${(1 - hsv.v) * 100}%` }}
            />
          </div>
          <input
            type="range"
            className="kb-colorpick-hue"
            min={0}
            max={359}
            value={Math.round(hsv.h)}
            aria-label="Hue"
            onChange={(e) => emit({ ...hsv, h: Number(e.target.value) })}
          />
          <input
            type="range"
            className="kb-colorpick-hue kb-colorpick-alpha"
            min={0}
            max={100}
            value={Math.round(alpha * 100)}
            aria-label="Opacity"
            onChange={(e) => emit(hsv, Number(e.target.value) / 100)}
          />
          <input
            className="kb-colorpick-hex"
            value={hexDraft}
            spellCheck={false}
            aria-label="Hex colour"
            onChange={(e) => setHexDraft(e.target.value)}
            onBlur={submitHex}
            onKeyDown={(e) => {
              if (e.key === 'Enter') submitHex()
            }}
          />
          {onConfirm && (
            <button
              type="button"
              className="kb-colorpick-confirm"
              aria-label="Add colour"
              onClick={() => {
                setOpen(false)
                onConfirm()
              }}
            >
              +
            </button>
          )}
        </div>
      )}
    </div>
  )
}

interface Hsv {
  h: number
  s: number
  v: number
}

/** `#rgb` / `#rrggbb` / `#rrggbbaa` (case-insensitive, `#` optional),
 * normalised with a leading `#`. */
export function normalizeHex(input: string): string | null {
  const raw = input.trim().replace(/^#/, '').toLowerCase()
  if (/^[0-9a-f]{6}$/.test(raw) || /^[0-9a-f]{8}$/.test(raw)) return `#${raw}`
  if (/^[0-9a-f]{3}$/.test(raw)) {
    return `#${raw.split('').map((c) => c + c).join('')}`
  }
  return null
}

function hexToHsv(hex: string): Hsv | null {
  const norm = normalizeHex(hex)
  if (!norm) return null
  const r = parseInt(norm.slice(1, 3), 16) / 255
  const g = parseInt(norm.slice(3, 5), 16) / 255
  const b = parseInt(norm.slice(5, 7), 16) / 255
  const max = Math.max(r, g, b)
  const min = Math.min(r, g, b)
  const d = max - min
  let h = 0
  if (d !== 0) {
    if (max === r) h = 60 * (((g - b) / d) % 6)
    else if (max === g) h = 60 * ((b - r) / d + 2)
    else h = 60 * ((r - g) / d + 4)
  }
  if (h < 0) h += 360
  return { h, s: max === 0 ? 0 : d / max, v: max }
}

function hsvToHex({ h, s, v }: Hsv): string {
  const c = v * s
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1))
  const m = v - c
  const [r, g, b] =
    h < 60 ? [c, x, 0]
    : h < 120 ? [x, c, 0]
    : h < 180 ? [0, c, x]
    : h < 240 ? [0, x, c]
    : h < 300 ? [x, 0, c]
    : [c, 0, x]
  const to = (n: number) =>
    Math.round((n + m) * 255)
      .toString(16)
      .padStart(2, '0')
  return `#${to(r)}${to(g)}${to(b)}`
}
