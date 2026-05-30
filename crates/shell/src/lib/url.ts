export function normaliseUrl(input: string): string | null {
  const trimmed = input.trim()
  if (!trimmed) return null

  try {
    const u = new URL(trimmed)
    // Reject things like `mailto:` here.
    if (u.protocol === 'http:' || u.protocol === 'https:' || u.protocol === 'about:') {
      return u.toString()
    }
  } catch {
    /* fall through */
  }

  if (looksLikeHost(trimmed)) {
    return `https://${trimmed}`
  }

  return `https://duckduckgo.com/?q=${encodeURIComponent(trimmed)}`
}

const HOST_RE = /^[A-Za-z0-9.\-_]+(:\d+)?(\/.*)?$/

function looksLikeHost(input: string): boolean {
  if (input.includes(' ')) return false
  if (!input.includes('.')) return false
  return HOST_RE.test(input)
}

/** Truthy when a tab's URL should not cause a native webview to be created. */
export function isChromeUrl(url: string): boolean {
  return url.startsWith('about:')
}

export function tabDisplayName(tab: { title: string; url: string }): string {
  if (tab.title && tab.title !== tab.url) {
    return tab.title
  }
  try {
    const u = new URL(tab.url)
    if (u.protocol === 'about:') {
      return 'Launchpad'
    }
    return u.host.replace(/^www\./, '')
  } catch {
    return tab.title || tab.url
  }
}
