export const USAGE_PANEL_WIDTH_MIN = 280
export const USAGE_PANEL_WIDTH_MAX = 520
export const USAGE_PANEL_WIDTH_DEFAULT = 320

const WEEKDAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat']

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
  const [year, month, date] = day.split('-').map(Number)
  return WEEKDAYS[new Date(year, month - 1, date).getDay()]
}

/** Within the panel bounds and half the window. */
export function clampUsagePanelWidth(width: number, viewport: number): number {
  const max = Math.min(USAGE_PANEL_WIDTH_MAX, Math.round(viewport * 0.5))
  return Math.max(USAGE_PANEL_WIDTH_MIN, Math.min(Number.isFinite(width) ? width : 0, max))
}
