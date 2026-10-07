/** Reduce a pasted URL or `host/path` to a `www.`-stripped, lowercased host. */
export function normalizeHostInput(raw: string): string {
  const s = raw.trim().toLowerCase()
  if (s === '') return ''
  // Parsing drops a scheme, path and port alike.
  try {
    const hasScheme = /^[a-z][a-z0-9+.-]*:\/\//.test(s)
    const host = new URL(hasScheme ? s : `http://${s}`).hostname
    return host.replace(/^www\./, '')
  } catch {
    return ''
  }
}
