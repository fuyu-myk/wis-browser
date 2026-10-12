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

function parseDay(day: string): Date {
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

/** The Monday-to-Sunday week holding `date`. */
export function weekOf(date: Date): { from: string; to: string } {
  const sinceMonday = (date.getDay() + 6) % 7
  const monday = new Date(date.getFullYear(), date.getMonth(), date.getDate() - sinceMonday)
  const sunday = new Date(monday.getFullYear(), monday.getMonth(), monday.getDate() + 6)
  return { from: localDay(monday), to: localDay(sunday) }
}

/** The week `weeks` away from the one starting on `from`. */
export function shiftWeek(from: string, weeks: number): { from: string; to: string } {
  const start = parseDay(from)
  return weekOf(new Date(start.getFullYear(), start.getMonth(), start.getDate() + weeks * 7))
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

/** Average over the days up to and including `today`. */
export function dailyAverage(days: ReadonlyArray<DaySeconds>, today: string): number {
  const started = days.filter((d) => d.day <= today)
  if (started.length === 0) return 0
  return started.reduce((sum, d) => sum + d.seconds, 0) / started.length
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
