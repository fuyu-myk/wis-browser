import { useQuery, useQueryClient } from '@tanstack/react-query'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'

import { commands } from '@/bindings/commands'
import type { UsageSummary } from '@/bindings/types'
import { Segmented } from '@/components/controls'
import { SiteFavicon } from '@/components/SiteFavicon'
import { UsageBars } from '@/components/UsageBars'
import { useDockWidth } from '@/hooks/useDockWidth'
import { cx } from '@/lib/cx'
import { errorMessage } from '@/lib/errors'
import { LockIcon, XIcon } from '@/lib/icons'
import {
  clampUsagePanelWidth,
  formatDuration,
  hourLabel,
  localDay,
  sharePercent,
  USAGE_PANEL_WIDTH_DEFAULT,
  weekdayLabel,
  weekOf,
} from '@/lib/usage'

type Range = 'today' | 'week'

const RANGES = [
  { value: 'today' as const, label: 'Today' },
  { value: 'week' as const, label: 'This week' },
]
const HOUR_TICKS = [0, 6, 12, 18].map((index) => ({ index, label: hourLabel(index) }))
const HOUR_LABELS = Array.from({ length: 24 }, (_, hour) => hourLabel(hour))
const SITE_LIMIT = 25

const clampWidth = (width: number) => clampUsagePanelWidth(width, window.innerWidth)

interface UsagePanelProps {
  open: boolean
  /** Docked beside the sidebar, opposite the assistant. */
  side: 'left' | 'right'
  tracking: boolean
  onClose: () => void
  onOpenSettings: () => void
}

/** Today's or this week's active time per site, docked beside the page. */
export function UsagePanel({ open, side, tracking, onClose, onOpenSettings }: UsagePanelProps) {
  const [range, setRange] = useState<Range>('today')
  const [now, setNow] = useState(() => new Date())
  const { width, startResize } = useDockWidth(
    'wis-usage-panel-width',
    USAGE_PANEL_WIDTH_DEFAULT,
    clampWidth,
    side,
  )
  const queryClient = useQueryClient()

  useEffect(() => {
    if (!open) return
    setNow(new Date())
    const timer = window.setInterval(() => setNow(new Date()), 60_000)
    const unlisten = listen('usage-changed', () => {
      void queryClient.invalidateQueries({ queryKey: ['usage'] })
    }).catch(() => null)
    return () => {
      window.clearInterval(timer)
      void unlisten.then((stop) => stop?.())
    }
  }, [open, queryClient])

  const span =
    range === 'today' ? { from: localDay(now), to: localDay(now) } : weekOf(now)
  const { data, error, isPlaceholderData } = useQuery({
    queryKey: ['usage', range, span.from, span.to],
    queryFn: () => commands.usageSummary(span.from, span.to),
    enabled: open && tracking,
    placeholderData: (previous, previousQuery) =>
      previousQuery?.queryKey[1] === range ? previous : undefined,
  })

  if (!open) return null

  return (
    <aside
      className={cx('kb-ai-panel', 'kb-usage-panel', `is-${side}`)}
      style={{ width: `${width}px` }}
      aria-label="Usage"
      onKeyDown={(e) => {
        if (e.key !== 'Escape') return
        e.preventDefault()
        onClose()
      }}
    >
      <div
        className="kb-ai-grip"
        onPointerDown={startResize}
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize usage"
      />
      <div className="kb-ai-head">
        <span className="kb-ai-title">Usage</span>
        <div className="kb-ai-head-actions">
          <button
            type="button"
            className="kb-ai-btn kb-ai-icon"
            onClick={onClose}
            aria-label="Close usage"
            title="Close usage"
          >
            <XIcon />
          </button>
        </div>
      </div>

      <div className="kb-usage-scroll">
        <Segmented value={range} onChange={setRange} options={RANGES} ariaLabel="Range" />
        {!tracking ? (
          <div className="kb-usage-empty">
            <p>Usage tracking is off.</p>
            <button type="button" className="kb-ai-btn" onClick={onOpenSettings}>
              Open settings
            </button>
          </div>
        ) : error ? (
          <p className="kb-usage-empty">{errorMessage(error)}</p>
        ) : data ? (
          <div className={cx('kb-usage-body', { 'is-stale': isPlaceholderData })}>
            <UsageBody summary={data} range={range} now={now} />
          </div>
        ) : null}
        <p className="kb-usage-note">
          <LockIcon />
          Counted on this device only
        </p>
      </div>
    </aside>
  )
}

function UsageBody({ summary, range, now }: { summary: UsageSummary; range: Range; now: Date }) {
  const [showAll, setShowAll] = useState(false)
  const today = localDay(now)
  const sites = showAll ? summary.sites : summary.sites.slice(0, SITE_LIMIT)
  const total = summary.total_seconds
  const chart =
    range === 'today'
      ? {
          values: summary.hours,
          labels: HOUR_LABELS,
          ticks: HOUR_TICKS,
          centered: false,
          current: now.getHours(),
          caption: 'Time per hour today',
        }
      : {
          values: summary.days.map((d) => d.seconds),
          labels: summary.days.map((d) => weekdayLabel(d.day)),
          ticks: summary.days.map((d, index) => ({ index, label: weekdayLabel(d.day) })),
          centered: true,
          current: summary.days.findIndex((d) => d.day === today),
          caption: 'Time per day this week',
        }

  return (
    <>
      <div className="kb-usage-hero">
        <span className="kb-usage-total">{formatDuration(total)}</span>
        <span className="kb-usage-caption">
          on the web {range === 'today' ? 'today' : 'this week'}
        </span>
      </div>
      <UsageBars {...chart} current={chart.current < 0 ? null : chart.current} />
      {summary.sites.length === 0 ? (
        <p className="kb-usage-empty">
          No time counted {range === 'today' ? 'today' : 'this week'} yet.
        </p>
      ) : (
        <>
          <div className="kb-usage-heading">Sites</div>
          <ul className="kb-usage-sites">
            {sites.map((entry) => {
              const share = sharePercent(entry.seconds, total)
              return (
                <li key={entry.site} className="kb-usage-site">
                  <span className="kb-usage-fav">
                    <SiteFavicon site={entry.site} />
                  </span>
                  <div className="kb-usage-site-main">
                    <div className="kb-usage-site-line">
                      <span className="kb-usage-site-name">{entry.site}</span>
                      <span className="kb-usage-site-time">
                        {formatDuration(entry.seconds)} · {share}%
                      </span>
                    </div>
                    <div className="kb-usage-share" aria-hidden="true">
                      <span style={{ width: `${share}%` }} />
                    </div>
                  </div>
                </li>
              )
            })}
          </ul>
          {sites.length < summary.sites.length && (
            <button type="button" className="kb-ai-btn" onClick={() => setShowAll(true)}>
              Show all {summary.sites.length} sites
            </button>
          )}
        </>
      )}
    </>
  )
}
