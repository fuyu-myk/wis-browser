import { useRef, useState, type KeyboardEvent } from 'react'

import { cx } from '@/lib/cx'
import { formatDuration } from '@/lib/usage'

const WIDTH = 288
const HEIGHT = 96
const MAX_BAR = 24
const RADIUS = 4

interface UsageBarsProps {
  /** Seconds per slot. */
  values: ReadonlyArray<number>
  /** Each slot's name in the readout, e.g. "2 PM" or "Mon". */
  labels: ReadonlyArray<string>
  ticks: ReadonlyArray<{ index: number; label: string }>
  /** Ticks sit under the middle of their slot rather than at its start. */
  centered: boolean
  /** The slot holding now, drawn in the accent. */
  current: number | null
  caption: string
}

/** Rounded at the data end, square at the baseline. */
function barPath(x: number, y: number, width: number, height: number): string {
  const r = Math.min(RADIUS, width / 2, height)
  return (
    `M${x},${y + height}V${y + r}A${r},${r} 0 0 1 ${x + r},${y}` +
    `H${x + width - r}A${r},${r} 0 0 1 ${x + width},${y + r}V${y + height}Z`
  )
}

/**
 * One tab stop: arrow keys, Home and End step through the slots, and the
 * readout line says the focused or hovered slot's value.
 */
export function UsageBars({ values, labels, ticks, centered, current, caption }: UsageBarsProps) {
  const [active, setActive] = useState<number | null>(null)
  const focused = useRef(false)
  const max = Math.max(1, ...values)
  const slot = WIDTH / values.length
  const barWidth = Math.min(MAX_BAR, Math.max(2, slot - 4))
  const busiest = values.indexOf(Math.max(...values))
  const last = values.length - 1

  const readout =
    active !== null
      ? `${labels[active]} · ${formatDuration(values[active])}`
      : values[busiest] > 0
        ? `Busiest ${labels[busiest]} · ${formatDuration(values[busiest])}`
        : ''

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const from = active ?? current ?? busiest
    const next =
      e.key === 'ArrowLeft'
        ? Math.max(0, from - 1)
        : e.key === 'ArrowRight'
          ? Math.min(last, from + 1)
          : e.key === 'Home'
            ? 0
            : e.key === 'End'
              ? last
              : null
    if (next === null) return
    e.preventDefault()
    setActive(next)
  }

  return (
    <div className="kb-usage-chart">
      <div className="kb-usage-readout" aria-live="polite">
        {readout}
      </div>
      <div
        className="kb-usage-plot"
        tabIndex={0}
        role="group"
        aria-label={`${caption}. Use the arrow keys to step through.`}
        onKeyDown={onKeyDown}
        onFocus={() => {
          focused.current = true
          setActive((prev) => prev ?? current ?? busiest)
        }}
        onBlur={() => {
          focused.current = false
          setActive(null)
        }}
        onPointerLeave={() => {
          if (!focused.current) setActive(null)
        }}
      >
        <svg className="kb-usage-bars" viewBox={`0 0 ${WIDTH} ${HEIGHT}`} aria-hidden="true">
          <line className="kb-usage-axis" x1={0} x2={WIDTH} y1={HEIGHT - 0.5} y2={HEIGHT - 0.5} />
          {values.map((value, i) => {
            const height = value > 0 ? Math.max(3, (value / max) * (HEIGHT - 4)) : 2
            return (
              <g
                key={labels[i]}
                className={cx('kb-usage-slot', {
                  'is-now': i === current,
                  'is-active': i === active,
                })}
                onPointerEnter={() => setActive(i)}
              >
                <rect className="kb-usage-hit" x={i * slot} y={0} width={slot} height={HEIGHT} />
                <path
                  className="kb-usage-bar"
                  d={barPath(i * slot + (slot - barWidth) / 2, HEIGHT - height, barWidth, height)}
                />
              </g>
            )
          })}
        </svg>
      </div>
      <div className={cx('kb-usage-ticks', { 'is-centered': centered })} aria-hidden="true">
        {ticks.map((tick) => (
          <span
            key={tick.index}
            style={{ left: `${((tick.index + (centered ? 0.5 : 0)) / values.length) * 100}%` }}
          >
            {tick.label}
          </span>
        ))}
      </div>
    </div>
  )
}
