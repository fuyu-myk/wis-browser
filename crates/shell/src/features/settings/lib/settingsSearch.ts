export interface SettingEntry {
  section: string
  title: string
  detail: string
}

const RESULT_LIMIT = 12

function rank(entry: SettingEntry, query: string, terms: ReadonlyArray<string>): number {
  const title = entry.title.toLowerCase()
  if (title.startsWith(query)) return 0
  if (terms.every((term) => title.includes(term))) return 1
  return 2
}

/**
 * Indices of entries holding every word of `query` in their title, detail or
 * section: titles starting with it first, then title matches, then the rest.
 */
export function searchSettings(
  entries: ReadonlyArray<SettingEntry>,
  query: string,
  limit = RESULT_LIMIT,
): number[] {
  const normalized = query.trim().toLowerCase()
  const terms = normalized.split(/\s+/).filter(Boolean)
  if (terms.length === 0) return []

  return entries
    .map((entry, index) => ({ entry, index }))
    .filter(({ entry }) => {
      const text = `${entry.title} ${entry.detail} ${entry.section}`.toLowerCase()
      return terms.every((term) => text.includes(term))
    })
    .map(({ entry, index }) => ({ index, rank: rank(entry, normalized, terms) }))
    .sort((a, b) => a.rank - b.rank || a.index - b.index)
    .slice(0, limit)
    .map(({ index }) => index)
}
