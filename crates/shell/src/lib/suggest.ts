export interface Suggestion {
  value: string
  label?: string
}

/**
 * The suggestions a field lists: all of them until the user types, then those
 * whose value or label contains the input, leaving out an exact match alone.
 */
export function matchSuggestions<T extends Suggestion>(items: readonly T[], text: string, typed: boolean): T[] {
  const query = text.trim().toLowerCase()
  if (!typed || query === '') return [...items]
  const found = items.filter(
    (item) => item.value.toLowerCase().includes(query) || (item.label ?? '').toLowerCase().includes(query),
  )
  return found.length === 1 && found[0].value.toLowerCase() === query ? [] : found
}
