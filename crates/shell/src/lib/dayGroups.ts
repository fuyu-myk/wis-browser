export interface DayGroup<T> {
  key: string
  label: string
  entries: T[]
}

/** Groups newest-first entries by the local calendar day `at` gives. */
export function groupByDay<T>(entries: ReadonlyArray<T>, at: (entry: T) => string): DayGroup<T>[] {
  const groups: DayGroup<T>[] = []
  let current: DayGroup<T> | null = null
  for (const entry of entries) {
    const date = new Date(at(entry))
    const key = dayKey(date)
    if (!current || current.key !== key) {
      current = { key, label: dayLabel(date), entries: [] }
      groups.push(current)
    }
    current.entries.push(entry)
  }
  return groups
}

function dayKey(date: Date): string {
  return `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`
}

function dayLabel(date: Date): string {
  const today = new Date()
  const yesterday = new Date()
  yesterday.setDate(today.getDate() - 1)
  if (dayKey(date) === dayKey(today)) return 'Today'
  if (dayKey(date) === dayKey(yesterday)) return 'Yesterday'
  return date.toLocaleDateString(undefined, {
    weekday: 'long',
    month: 'long',
    day: 'numeric',
    year: date.getFullYear() === today.getFullYear() ? undefined : 'numeric',
  })
}

export function formatTime(iso: string): string {
  return new Date(iso).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })
}

export function hostOf(url: string): string {
  try {
    return new URL(url).host.replace(/^www\./, '')
  } catch {
    return url
  }
}
