import { useSlotCursor } from '@/hooks/useSlotCursor'
import { cx } from '@/lib/cx'
import { durationTicks, formatDuration } from '@/lib/usage'

const WIDTH = 288
const HEIGHT = 96
const PAD_TOP = 4
const MAX_BAR = 24
const RADIUS = 4
const GRID_INTERVALS = 2

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
  const grid = durationTicks(Math.max(0, ...values), GRID_INTERVALS)
  const top = grid[grid.length - 1]
  const y = (seconds: number) => Math.min(HEIGHT - 0.5, HEIGHT - (seconds / top) * (HEIGHT - PAD_TOP))
  const slot = WIDTH / values.length
  const barWidth = Math.min(MAX_BAR, Math.max(2, slot - 4))
  const busiest = values.indexOf(Math.max(...values))
  const { active, hover, plotProps } = useSlotCursor(values.length - 1, current ?? busiest)

  const readout =
    active !== null
      ? `${labels[active]} · ${formatDuration(values[active])}`
      : values[busiest] > 0
        ? `Busiest ${labels[busiest]} · ${formatDuration(values[busiest])}`
        : ''

  return (
    <div className="kb-usage-chart">
      <div className="kb-usage-readout" aria-live="polite">
        {readout}
      </div>
      <div className="kb-usage-frame">
        <div
          className="kb-usage-plot"
          role="group"
          aria-label={`${caption}. Use the arrow keys to step through.`}
          {...plotProps}
        >
          <svg className="kb-usage-bars" viewBox={`0 0 ${WIDTH} ${HEIGHT}`} aria-hidden="true">
            {grid.map((tick) => (
              <line
                key={tick}
                className={cx('kb-usage-grid', { 'is-base': tick === 0 })}
                x1={0}
                x2={WIDTH}
                y1={y(tick)}
                y2={y(tick)}
                vectorEffect="non-scaling-stroke"
              />
            ))}
            {values.map((value, i) => {
              const height = value > 0 ? Math.max(3, HEIGHT - y(value)) : 2
              return (
                <g
                  key={labels[i]}
                  className={cx('kb-usage-slot', {
                    'is-now': i === current,
                    'is-active': i === active,
                  })}
                  onPointerEnter={() => hover(i)}
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
        <div className="kb-usage-yticks" aria-hidden="true">
          {grid.slice(1).map((tick) => (
            <span key={tick} style={{ top: `${(y(tick) / HEIGHT) * 100}%` }}>
              {formatDuration(tick)}
            </span>
          ))}
        </div>
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
