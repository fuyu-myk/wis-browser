import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { commands } from '@/bindings/commands'
import type { Tab, UsageSummary } from '@/bindings/types'
import { UsageChartCard } from '@/features/usage/components/UsageChartCard'
import { UsageSites } from '@/features/usage/components/UsageSites'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useNavigateOrCreateTab } from '@/hooks/useNavigateOrCreateTab'
import { useScrollMemory } from '@/hooks/useScrollMemory'
import { useUsageClock } from '@/features/usage/hooks/useUsageClock'
import { cx } from '@/lib/cx'
import { errorMessage } from '@/lib/errors'
import { ChevronLeftIcon, ChevronRightIcon, LockIcon } from '@/lib/icons'
import { SETTINGS_URL } from '@/lib/url'
import {
  changeLabel,
  dailyAverage,
  dayLabel,
  firstWeekday,
  formatDuration,
  localDay,
  parseDay,
  shiftWeek,
  weekChange,
  weekdayLabel,
  weekOf,
  weekRangeLabel,
  weekTotals,
  weeklyAverage,
  type DaySeconds,
} from '@/features/usage/lib/usage'

const TREND_WEEKS = 8

/** Weekly active time per day and per site (`about:usage`). */
export function UsagePage({ activeTab }: { activeTab: Tab }) {
  const { data: state } = useBrowserState()
  const tracking = state?.settings.usage_tracking ?? false
  const now = useUsageClock(true)
  const [anchor, setAnchor] = useState<string | null>(null)
  const navigate = useNavigateOrCreateTab(state?.active_workspace_id ?? null, activeTab)
  const attachScroll = useScrollMemory(activeTab.id)

  const firstDay = firstWeekday(state?.settings.week_start ?? 'auto')
  const current = weekOf(now, firstDay)
  const week = anchor === null ? current : weekOf(parseDay(anchor), firstDay)
  const isCurrent = week.from >= current.from
  const stepWeek = (weeks: number) => {
    const next = shiftWeek(week.from, weeks)
    setAnchor(next.from >= current.from ? null : next.from)
  }
  const label = weekRangeLabel(week.from, week.to, now)
  const trendFrom = shiftWeek(week.from, 1 - TREND_WEEKS).from
  const { data, error, isPlaceholderData } = useQuery({
    queryKey: ['usage', 'page', week.from, week.to],
    queryFn: async () => {
      const [summary, trend] = await Promise.all([
        commands.usageSummary(week.from, week.to),
        commands.usageSummary(trendFrom, week.to),
      ])
      return { summary, trend: trend.days }
    },
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
              <WeekSummary
                summary={data.summary}
                trend={data.trend}
                now={now}
                isCurrent={isCurrent}
                label={label}
              />
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
  /** Every day of the trend's weeks, ending with this one. */
  trend: ReadonlyArray<DaySeconds>
  now: Date
  isCurrent: boolean
  label: string
}

function WeekSummary({ summary, trend, now, isCurrent, label }: WeekSummaryProps) {
  const today = localDay(now)
  const total = summary.total_seconds
  const busiest = summary.days.reduce(
    (best, d) => (best === null || d.seconds > best.seconds ? d : best),
    null as DaySeconds | null,
  )
  const change = weekChange(trend.slice(-7), trend.slice(-14, -7), today)
  const weeks = weekTotals(trend)
  const todayIndex = summary.days.findIndex((d) => d.day === today)
  const average = dailyAverage(summary.days, today)
  const running = isCurrent ? weeks[weeks.length - 1]?.day : undefined
  const weekTitle = (day: string) =>
    `${weekRangeLabel(day, shiftWeek(day, 0).to, now)}${day === running ? ' so far' : ''}`

  return (
    <>
      <div className="kb-usage-stats">
        <Stat
          label="Total"
          value={formatDuration(total)}
          detail={change === null ? undefined : changeLabel(change, isCurrent)}
        />
        <Stat label="Daily average" value={formatDuration(average)} />
        <Stat
          label="Busiest day"
          value={busiest && busiest.seconds > 0 ? dayLabel(busiest.day) : 'None yet'}
          detail={busiest && busiest.seconds > 0 ? formatDuration(busiest.seconds) : undefined}
        />
      </div>

      <UsageChartCard
        heading="Time per day"
        unit="Day"
        points={summary.days}
        shown={summary.days.filter((d) => d.day <= today).length}
        current={todayIndex < 0 ? null : todayIndex}
        label={weekdayLabel}
        title={dayLabel}
        average={average > 0 ? average : null}
        caption={`Time per day, ${label}`}
        total={total}
      />

      <UsageChartCard
        heading={`Last ${TREND_WEEKS} weeks`}
        unit="Week"
        points={weeks}
        shown={weeks.length}
        current={isCurrent ? weeks.length - 1 : null}
        axisTitle="Week"
        title={weekTitle}
        average={weeklyAverage(weeks, isCurrent)}
        caption={`Time per week, ${TREND_WEEKS} weeks to ${label}`}
      />

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
