import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { commands } from '@/bindings/commands'
import type { Tab, UsageSummary } from '@/bindings/types'
import { Segmented } from '@/components/controls'
import { UsageLine } from '@/components/UsageLine'
import { UsageSites } from '@/components/UsageSites'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useNavigateOrCreateTab } from '@/hooks/useNavigateOrCreateTab'
import { useScrollMemory } from '@/hooks/useScrollMemory'
import { useUsageClock } from '@/hooks/useUsageClock'
import { cx } from '@/lib/cx'
import { errorMessage } from '@/lib/errors'
import { ChevronLeftIcon, ChevronRightIcon, LockIcon } from '@/lib/icons'
import { SETTINGS_URL } from '@/lib/url'
import {
  dailyAverage,
  dayLabel,
  formatDuration,
  localDay,
  sharePercent,
  shiftWeek,
  weekOf,
  weekRangeLabel,
} from '@/lib/usage'

type View = 'chart' | 'table'

const VIEWS = [
  { value: 'chart' as const, label: 'Chart' },
  { value: 'table' as const, label: 'Table' },
]

/** Weekly active time per day and per site (`about:usage`). */
export function UsagePage({ activeTab }: { activeTab: Tab }) {
  const { data: state } = useBrowserState()
  const tracking = state?.settings.usage_tracking ?? false
  const now = useUsageClock(true)
  const [anchor, setAnchor] = useState<string | null>(null)
  const navigate = useNavigateOrCreateTab(state?.active_workspace_id ?? null, activeTab)
  const attachScroll = useScrollMemory(activeTab.id)

  const current = weekOf(now)
  const week = anchor === null ? current : shiftWeek(anchor, 0)
  const isCurrent = week.from >= current.from
  const stepWeek = (weeks: number) => {
    const next = shiftWeek(week.from, weeks)
    setAnchor(next.from >= current.from ? null : next.from)
  }
  const label = weekRangeLabel(week.from, week.to, now)
  const { data, error, isPlaceholderData } = useQuery({
    queryKey: ['usage', 'week', week.from, week.to],
    queryFn: () => commands.usageSummary(week.from, week.to),
    enabled: tracking,
    placeholderData: keepPreviousData,
  })

  return (
    <div className="kb-history-page kb-usage-page">
      <div className="inner">
        <header className="kb-history-head">
          <div className="kb-history-head-row">
            <h1>Usage</h1>
            <div className="kb-usage-week">
              {!isCurrent && (
                <button type="button" className="kb-history-btn" onClick={() => setAnchor(null)}>
                  This week
                </button>
              )}
              <button
                type="button"
                className="kb-usage-step"
                onClick={() => stepWeek(-1)}
                aria-label="Previous week"
                title="Previous week"
              >
                <ChevronLeftIcon />
              </button>
              <span className="kb-usage-week-label">{label}</span>
              <button
                type="button"
                className="kb-usage-step"
                onClick={() => stepWeek(1)}
                disabled={isCurrent}
                aria-label="Next week"
                title="Next week"
              >
                <ChevronRightIcon />
              </button>
            </div>
          </div>
        </header>

        <div className="kb-usage-page-body" ref={attachScroll}>
          {!tracking ? (
            <div className="kb-usage-empty">
              <p>Usage tracking is off.</p>
              <button
                type="button"
                className="kb-history-btn"
                onClick={() => navigate(SETTINGS_URL)}
              >
                Open settings
              </button>
            </div>
          ) : error ? (
            <p className="kb-history-empty">{errorMessage(error)}</p>
          ) : data ? (
            <div className={cx('kb-usage-week-body', { 'is-stale': isPlaceholderData })}>
              <WeekSummary summary={data} today={localDay(now)} caption={`Time per day, ${label}`} />
            </div>
          ) : null}
          <p className="kb-usage-note">
            <LockIcon />
            Counted on this device only
          </p>
        </div>
      </div>
    </div>
  )
}

interface WeekSummaryProps {
  summary: UsageSummary
  today: string
  caption: string
}

function WeekSummary({ summary, today, caption }: WeekSummaryProps) {
  const [view, setView] = useState<View>('chart')
  const total = summary.total_seconds
  const started = summary.days.filter((d) => d.day <= today)
  const busiest = summary.days.reduce(
    (best, d) => (best === null || d.seconds > best.seconds ? d : best),
    null as UsageSummary['days'][number] | null,
  )

  return (
    <>
      <div className="kb-usage-stats">
        <Stat label="Total" value={formatDuration(total)} />
        <Stat label="Daily average" value={formatDuration(dailyAverage(summary.days, today))} />
        <Stat
          label="Busiest day"
          value={busiest && busiest.seconds > 0 ? dayLabel(busiest.day) : 'None yet'}
          detail={busiest && busiest.seconds > 0 ? formatDuration(busiest.seconds) : undefined}
        />
      </div>

      <section className="kb-usage-card">
        <div className="kb-usage-card-head">
          <h2 className="kb-usage-heading">Time per day</h2>
          <Segmented value={view} onChange={setView} options={VIEWS} ariaLabel="View" />
        </div>
        {view === 'chart' ? (
          <UsageLine key={summary.days[0]?.day} days={summary.days} today={today} caption={caption} />
        ) : (
          <table className="kb-usage-table">
            <thead>
              <tr>
                <th scope="col">Day</th>
                <th scope="col">Time</th>
                <th scope="col">Share</th>
              </tr>
            </thead>
            <tbody>
              {started.map((d) => (
                <tr key={d.day} className={cx({ 'is-now': d.day === today })}>
                  <th scope="row">{dayLabel(d.day)}</th>
                  <td>{formatDuration(d.seconds)}</td>
                  <td>{sharePercent(d.seconds, total)}%</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>

      <section className="kb-usage-card">
        <h2 className="kb-usage-heading">Sites</h2>
        {summary.sites.length === 0 ? (
          <p className="kb-usage-empty">No time counted.</p>
        ) : (
          <UsageSites sites={summary.sites} total={total} />
        )}
      </section>
    </>
  )
}

function Stat({ label, value, detail }: { label: string; value: string; detail?: string }) {
  return (
    <div className="kb-usage-stat">
      <span className="kb-usage-stat-label">{label}</span>
      <span className="kb-usage-stat-value">{value}</span>
      {detail && <span className="kb-usage-stat-detail">{detail}</span>}
    </div>
  )
}
