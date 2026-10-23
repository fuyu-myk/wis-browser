import type { WeekStart } from '@/bindings/types'

export const USAGE_PANEL_WIDTH_MIN = 280
export const USAGE_PANEL_WIDTH_MAX = 520
export const USAGE_PANEL_WIDTH_DEFAULT = 320

const WEEKDAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat']
const TICK_STEPS = [5, 10, 15, 30, 60, 120, 180, 240, 360, 480, 720, 1440].map((m) => m * 60)
const TICK_INTERVALS = 4

export interface DaySeconds {
  day: string
  seconds: number
}

export function parseDay(day: string): Date {
  const [year, month, date] = day.split('-').map(Number)
  return new Date(year, month - 1, date)
}

/** "45s", "12m", "1h 5m". */
export function formatDuration(seconds: number): string {
  const total = Math.max(0, Math.round(seconds))
  if (total < 60) return `${total}s`
  const minutes = Math.floor(total / 60)
  if (minutes < 60) return `${minutes}m`
  const hours = Math.floor(minutes / 60)
  const rest = minutes % 60
  return rest === 0 ? `${hours}h` : `${hours}h ${rest}m`
}

/** The local calendar date, as `YYYY-MM-DD`. */
export function localDay(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`
}

type WeekInfoLocale = Intl.Locale & {
  getWeekInfo?: () => { firstDay: number }
  weekInfo?: { firstDay: number }
}

/** The week's first day as `Date.getDay()` counts (0 is Sunday); Monday when the region is unknown. */
export function firstWeekday(start: WeekStart, locale?: string): number {
  if (start === 'monday') return 1
  if (start === 'sunday') return 0
  try {
    const region = new Intl.Locale(
      locale ?? Intl.DateTimeFormat().resolvedOptions().locale,
    ) as WeekInfoLocale
    const info = region.getWeekInfo?.() ?? region.weekInfo
    return info ? info.firstDay % 7 : 1
  } catch {
    return 1
  }
}

/** The week holding `date`, starting on `firstDay` (0 is Sunday). */
export function weekOf(date: Date, firstDay = 1): { from: string; to: string } {
  const since = (date.getDay() - firstDay + 7) % 7
  return shiftWeek(localDay(new Date(date.getFullYear(), date.getMonth(), date.getDate() - since)), 0)
}

/** The week `weeks` away from the one starting on `from`. */
export function shiftWeek(from: string, weeks: number): { from: string; to: string } {
  const start = parseDay(from)
  const day = (offset: number) =>
    localDay(new Date(start.getFullYear(), start.getMonth(), start.getDate() + offset))
  return { from: day(weeks * 7), to: day(weeks * 7 + 6) }
}

/** A week's dates, with years only when the week is outside `now`'s year. */
export function weekRangeLabel(from: string, to: string, now: Date, locale?: string): string {
  const start = parseDay(from)
  const end = parseDay(to)
  const thisYear = start.getFullYear() === now.getFullYear() && end.getFullYear() === now.getFullYear()
  const format = new Intl.DateTimeFormat(locale, {
    month: 'short',
    day: 'numeric',
    ...(thisYear ? {} : { year: 'numeric' }),
  })
  return format.formatRange(start, end)
}

/** Style: "Mon, Oct 5". */
export function dayLabel(day: string, locale?: string): string {
  return new Intl.DateTimeFormat(locale, { weekday: 'short', month: 'short', day: 'numeric' }).format(
    parseDay(day),
  )
}

const sum = (days: ReadonlyArray<DaySeconds>) => days.reduce((total, d) => total + d.seconds, 0)

/** Each run of seven days summed under its first day; `days` starts on a week's first day. */
export function weekTotals(days: ReadonlyArray<DaySeconds>): DaySeconds[] {
  return Array.from({ length: Math.ceil(days.length / 7) }, (_, i) => {
    const week = days.slice(i * 7, i * 7 + 7)
    return { day: week[0].day, seconds: sum(week) }
  })
}

/** The mean of finished weeks since the first with any time, or null. */
export function weeklyAverage(weeks: ReadonlyArray<DaySeconds>, running: boolean): number | null {
  const finished = running ? weeks.slice(0, -1) : weeks
  const first = finished.findIndex((w) => w.seconds > 0)
  if (first < 0) return null
  const counted = finished.slice(first)
  return sum(counted) / counted.length
}

/**
 * The week's change from the one before over the same weekdays, as a
 * fraction; null when there is nothing to compare.
 */
export function weekChange(
  week: ReadonlyArray<DaySeconds>,
  before: ReadonlyArray<DaySeconds>,
  today: string,
): number | null {
  const started = week.filter((d) => d.day <= today).length
  const prior = sum(before.slice(0, started))
  if (started === 0 || prior === 0) return null
  return sum(week.slice(0, started)) / prior - 1
}

/** `partial` when the week is still running. */
export function changeLabel(change: number, partial: boolean): string {
  const percent = Math.round(Math.abs(change) * 100)
  const than = partial ? 'this time last week' : 'the week before'
  if (percent === 0) return `Same as ${than}`
  return `${percent}% ${change > 0 ? 'more' : 'less'} than ${than}`
}

/** Average over the days up to and including `today`. */
export function dailyAverage(days: ReadonlyArray<DaySeconds>, today: string): number {
  const started = days.filter((d) => d.day <= today)
  if (started.length === 0) return 0
  return sum(started) / started.length
}

/** Gridline values from zero: a round step, at most `intervals` of them, reaching `max`. */
export function durationTicks(max: number, intervals = TICK_INTERVALS): number[] {
  const step = TICK_STEPS.find((s) => s * intervals >= max) ?? TICK_STEPS[TICK_STEPS.length - 1]
  const count = Math.max(1, Math.ceil(max / step))
  return Array.from({ length: count + 1 }, (_, i) => i * step)
}

/** The slot a chart key moves to, or null for other keys. */
export function stepSlot(key: string, from: number, last: number): number | null {
  if (key === 'ArrowLeft') return Math.max(0, from - 1)
  if (key === 'ArrowRight') return Math.min(last, from + 1)
  if (key === 'Home') return 0
  if (key === 'End') return last
  return null
}

/** Whole percent of `total`; any time at all shows at least 1. */
export function sharePercent(seconds: number, total: number): number {
  if (total <= 0 || seconds <= 0) return 0
  return Math.max(1, Math.round((seconds / total) * 100))
}

export function hourLabel(hour: number): string {
  const suffix = hour < 12 ? 'AM' : 'PM'
  const twelve = hour % 12 === 0 ? 12 : hour % 12
  return `${twelve} ${suffix}`
}

/** Short weekday of a `YYYY-MM-DD` local date. */
export function weekdayLabel(day: string): string {
  return WEEKDAYS[parseDay(day).getDay()]
}

/** Within the panel bounds and half the window. */
export function clampUsagePanelWidth(width: number, viewport: number): number {
  const max = Math.min(USAGE_PANEL_WIDTH_MAX, Math.round(viewport * 0.5))
  return Math.max(USAGE_PANEL_WIDTH_MIN, Math.min(Number.isFinite(width) ? width : 0, max))
}
