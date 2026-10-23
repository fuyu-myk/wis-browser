import { useState } from 'react'

import { Segmented } from '@/components/controls'
import { UsageLine } from '@/features/usage/components/UsageLine'
import { cx } from '@/lib/cx'
import { formatDuration, sharePercent, type DaySeconds } from '@/features/usage/lib/usage'

type View = 'chart' | 'table'

const VIEWS = [
  { value: 'chart' as const, label: 'Chart' },
  { value: 'table' as const, label: 'Table' },
]

interface UsageChartCardProps {
  heading: string
  /** The table's first column, e.g. "Day". */
  unit: string
  points: ReadonlyArray<DaySeconds>
  shown: number
  current: number | null
  label?: (day: string) => string
  axisTitle?: string
  title: (day: string) => string
  average?: number | null
  caption: string
  /** Adds a share column against this total. */
  total?: number
}

/** A line chart that switches to the same points as a table. */
export function UsageChartCard({ heading, unit, total, ...line }: UsageChartCardProps) {
  const [view, setView] = useState<View>('chart')

  return (
    <section className="kb-usage-card">
      <div className="kb-usage-card-head">
        <h2 className="kb-usage-heading">{heading}</h2>
        {line.average != null && (
          <span className="kb-usage-legend">
            <svg width="18" height="4" aria-hidden="true">
              <line className="kb-usage-avg" x1={1} x2={17} y1={2} y2={2} />
            </svg>
            Avg {formatDuration(line.average)}
          </span>
        )}
        <Segmented value={view} onChange={setView} options={VIEWS} ariaLabel={`${heading} view`} />
      </div>
      <div className={cx('kb-usage-card-body', { 'is-table': view === 'table' })}>
        {view === 'chart' ? (
          <UsageLine key={line.points[0]?.day} {...line} />
        ) : (
          <table className="kb-usage-table">
            <thead>
              <tr>
                <th scope="col">{unit}</th>
                <th scope="col">Time</th>
                {total !== undefined && <th scope="col">Share</th>}
              </tr>
            </thead>
            <tbody>
              {line.points.slice(0, line.shown).map((d, i) => (
                <tr key={d.day} className={cx({ 'is-now': i === line.current })}>
                  <th scope="row">{line.title(d.day)}</th>
                  <td>{formatDuration(d.seconds)}</td>
                  {total !== undefined && <td>{sharePercent(d.seconds, total)}%</td>}
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </section>
  )
}
