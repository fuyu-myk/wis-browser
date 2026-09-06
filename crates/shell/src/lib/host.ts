/** Reduce a pasted URL or `host/path` to a `www.`-stripped, lowercased host. */
export function normalizeHostInput(raw: string): string {
  let s = raw.trim().toLowerCase()
  if (s === '') return ''
  if (s.includes('://')) {
    try {
      s = new URL(s).hostname
    } catch {
      // Not a parseable URL; fall through and treat the text as a host.
    }
  }
  s = s.split('/')[0]
  return s.replace(/^www\./, '')
}
