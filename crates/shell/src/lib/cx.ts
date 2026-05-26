/**
 * Class-name helper. Accepts strings, undefined/false (skipped), and
 * objects mapping classes to boolean conditions. Joins truthy entries with
 * a single space.
 */
export type ClassValue =
  | string
  | false
  | null
  | undefined
  | Record<string, boolean | null | undefined>
  | ClassValue[]

export function cx(...values: ClassValue[]): string {
  const out: string[] = []
  for (const v of values) {
    if (!v) continue
    if (typeof v === 'string') {
      out.push(v)
    } else if (Array.isArray(v)) {
      const nested = cx(...v)
      if (nested) out.push(nested)
    } else {
      for (const [key, cond] of Object.entries(v)) {
        if (cond) out.push(key)
      }
    }
  }
  return out.join(' ')
}
