import { useLayoutEffect, useRef, useState } from 'react'

import { useSlotCursor } from '@/hooks/useSlotCursor'
import { cx } from '@/lib/cx'
import {
  dayLabel,
  durationTicks,
  formatDuration,
  weekdayLabel,
  type DaySeconds,
} from '@/lib/usage'

const HEIGHT = 220
const PAD = { top: 14, right: 16, bottom: 30, left: 48 }
const DOT = 4
const DOT_ACTIVE = 6
const TIP_HALF = 64
const TIP_FLIP = 56

interface UsageLineProps {
  days: ReadonlyArray<DaySeconds>
  today: string
  caption: string
}

const crisp = (y: number) => Math.round(y) + 0.5

/**
 * Time per day as dots joined by a line. Days after `today` have no dot yet;
 * hover or the arrow keys show a day's value in the tooltip.
 */
export function UsageLine({ days, today, caption }: UsageLineProps) {
  const plotRef = useRef<HTMLDivElement>(null)
  const [width, setWidth] = useState(0)

  useLayoutEffect(() => {
    const plot = plotRef.current
    if (!plot) return
    const observer = new ResizeObserver(([entry]) => setWidth(entry.contentRect.width))
    observer.observe(plot)
    return () => observer.disconnect()
  }, [])

  const shown = days.filter((d) => d.day <= today)
  const last = shown.length - 1
  const todayIndex = days.findIndex((d) => d.day === today)
  const busiest = shown.reduce((best, d, i) => (d.seconds > shown[best].seconds ? i : best), 0)
  const { active, hover, plotProps } = useSlotCursor(
    Math.max(0, last),
    todayIndex >= 0 ? todayIndex : busiest,
  )

  const ticks = durationTicks(Math.max(0, ...shown.map((d) => d.seconds)))
  const top = ticks[ticks.length - 1]
  const plotWidth = Math.max(0, width - PAD.left - PAD.right)
  const plotHeight = HEIGHT - PAD.top - PAD.bottom
  const slot = plotWidth / Math.max(1, days.length)
  const base = PAD.top + plotHeight
  const x = (i: number) => PAD.left + slot * (i + 0.5)
  const y = (seconds: number) => PAD.top + plotHeight * (1 - seconds / top)

  const points = shown.map((d, i) => `${x(i)},${y(d.seconds)}`)
  const line = points.length > 0 ? `M${points.join('L')}` : ''
  const area = points.length > 0 ? `${line}L${x(last)},${base}L${x(0)},${base}Z` : ''
  const cursor = active !== null && active <= last ? active : null
  const tipHalf = Math.min(TIP_HALF, width / 2)

  return (
    <div className="kb-usage-line">
      <div
        ref={plotRef}
        className="kb-usage-plot"
        role="group"
        aria-label={`${caption}. Use the arrow keys to step through.`}
        {...plotProps}
      >
        {width > 0 && (
          <svg width={width} height={HEIGHT} aria-hidden="true">
            {ticks.map((tick) => (
              <g key={tick}>
                <line
                  className={cx('kb-usage-grid', { 'is-base': tick === 0 })}
                  x1={PAD.left}
                  x2={width - PAD.right}
                  y1={crisp(y(tick))}
                  y2={crisp(y(tick))}
                />
                <text className="kb-usage-ylabel" x={PAD.left - 8} y={y(tick)} dy="0.32em">
                  {tick === 0 ? '0' : formatDuration(tick)}
                </text>
              </g>
            ))}
            {days.map((d, i) => (
              <text
                key={d.day}
                className={cx('kb-usage-xlabel', { 'is-now': i === todayIndex })}
                x={x(i)}
                y={HEIGHT - 8}
              >
                {weekdayLabel(d.day)}
              </text>
            ))}
            <path className="kb-usage-area" d={area} />
            <path className="kb-usage-stroke" d={line} />
            {cursor !== null && (
              <line
                className="kb-usage-cross"
                x1={crisp(x(cursor))}
                x2={crisp(x(cursor))}
                y1={PAD.top}
                y2={base}
              />
            )}
            {shown.map((d, i) => (
              <circle
                key={d.day}
                className={cx('kb-usage-dot', {
                  'is-now': i === todayIndex,
                  'is-active': i === active,
                })}
                cx={x(i)}
                cy={y(d.seconds)}
                r={i === active ? DOT_ACTIVE : DOT}
              />
            ))}
            {shown.map((d, i) => (
              <rect
                key={d.day}
                className="kb-usage-hit"
                x={PAD.left + slot * i}
                y={0}
                width={slot}
                height={HEIGHT}
                onPointerEnter={() => hover(i)}
              />
            ))}
          </svg>
        )}
      </div>
      <div className="kb-usage-tip-layer" aria-live="polite">
        {cursor !== null && (
          <div
            className={cx('kb-usage-tip', { 'is-below': y(shown[cursor].seconds) < TIP_FLIP })}
            style={{
              left: Math.min(Math.max(x(cursor), tipHalf), width - tipHalf),
              top: y(shown[cursor].seconds),
            }}
          >
            <strong>{formatDuration(shown[cursor].seconds)}</strong>
            <span>{dayLabel(shown[cursor].day)}</span>
          </div>
        )}
      </div>
    </div>
  )
}
