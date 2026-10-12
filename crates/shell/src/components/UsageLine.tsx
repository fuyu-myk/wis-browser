import { useLayoutEffect, useRef, useState } from 'react'

import { useSlotCursor } from '@/hooks/useSlotCursor'
import { cx } from '@/lib/cx'
import { durationTicks, formatDuration, type DaySeconds } from '@/lib/usage'

const PAD = { top: 14, right: 16, bottom: 30, left: 48 }
const DOT = 4
const DOT_ACTIVE = 6
const TIP_HALF = 64
const TIP_FLIP = 56

interface UsageLineProps {
  points: ReadonlyArray<DaySeconds>
  /** How many points have been recorded; unrecorded ones have no dot yet. */
  shown: number
  /** The point holding now, in the emphasis step. */
  current: number | null
  /** Names each point under the axis. */
  label?: (day: string) => string
  /** A single `axisTitle` under the axis. */
  axisTitle?: string
  /** The tooltip's name for a point. */
  title: (day: string) => string
  /** Drawn as a dotted reference line. */
  average?: number | null
  caption: string
}

const crisp = (y: number) => Math.round(y) + 0.5

/** Dots joined by a line; hover or the arrow keys show a point in the tooltip. */
export function UsageLine({
  points,
  shown,
  current,
  label,
  axisTitle,
  title,
  average,
  caption,
}: UsageLineProps) {
  const plotRef = useRef<HTMLDivElement>(null)
  const [{ width, height }, setSize] = useState({ width: 0, height: 0 })

  useLayoutEffect(() => {
    const plot = plotRef.current
    if (!plot) return
    const observer = new ResizeObserver(([entry]) =>
      setSize({ width: entry.contentRect.width, height: entry.contentRect.height }),
    )
    observer.observe(plot)
    return () => observer.disconnect()
  }, [])

  const happened = points.slice(0, shown)
  const last = happened.length - 1
  const busiest = happened.reduce((best, d, i) => (d.seconds > happened[best].seconds ? i : best), 0)
  const { active, hover, plotProps } = useSlotCursor(Math.max(0, last), current ?? busiest)

  const ticks = durationTicks(Math.max(0, ...happened.map((d) => d.seconds)))
  const top = ticks[ticks.length - 1]
  const plotWidth = Math.max(0, width - PAD.left - PAD.right)
  const plotHeight = Math.max(0, height - PAD.top - PAD.bottom)
  const slot = plotWidth / Math.max(1, points.length)
  const base = PAD.top + plotHeight
  const x = (i: number) => PAD.left + slot * (i + 0.5)
  const y = (seconds: number) => PAD.top + plotHeight * (1 - seconds / top)

  const coords = happened.map((d, i) => `${x(i)},${y(d.seconds)}`)
  const line = coords.length > 0 ? `M${coords.join('L')}` : ''
  const area = coords.length > 0 ? `${line}L${x(last)},${base}L${x(0)},${base}Z` : ''
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
        {width > 0 && height > 0 && (
          <svg width={width} height={height} aria-hidden="true">
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
            {label
              ? points.map((d, i) => (
                  <text
                    key={d.day}
                    className={cx('kb-usage-xlabel', { 'is-now': i === current })}
                    x={x(i)}
                    y={height - 8}
                  >
                    {label(d.day)}
                  </text>
                ))
              : axisTitle && (
                  <text className="kb-usage-xlabel" x={PAD.left + plotWidth / 2} y={height - 8}>
                    {axisTitle}
                  </text>
                )}
            <path className="kb-usage-area" d={area} />
            {average != null && (
              <line
                className="kb-usage-avg"
                x1={PAD.left}
                x2={width - PAD.right}
                y1={y(average)}
                y2={y(average)}
              />
            )}
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
            {happened.map((d, i) => (
              <circle
                key={d.day}
                className={cx('kb-usage-dot', {
                  'is-now': i === current,
                  'is-active': i === active,
                })}
                cx={x(i)}
                cy={y(d.seconds)}
                r={i === active ? DOT_ACTIVE : DOT}
              />
            ))}
            {happened.map((d, i) => (
              <rect
                key={d.day}
                className="kb-usage-hit"
                x={PAD.left + slot * i}
                y={0}
                width={slot}
                height={height}
                onPointerEnter={() => hover(i)}
              />
            ))}
          </svg>
        )}
      </div>
      <div className="kb-usage-tip-layer" aria-live="polite">
        {cursor !== null && (
          <div
            className={cx('kb-usage-tip', { 'is-below': y(happened[cursor].seconds) < TIP_FLIP })}
            style={{
              left: Math.min(Math.max(x(cursor), tipHalf), width - tipHalf),
              top: y(happened[cursor].seconds),
            }}
          >
            <strong>{formatDuration(happened[cursor].seconds)}</strong>
            <span>{title(happened[cursor].day)}</span>
          </div>
        )}
      </div>
    </div>
  )
}
