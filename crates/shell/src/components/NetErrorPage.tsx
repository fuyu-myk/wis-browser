import { useState } from 'react'

import { commands } from '@/bindings/commands'
import type { LoadError, LoadErrorKind, Tab } from '@/bindings/types'

interface Copy {
  title: string
  detail: string
}

function copyFor(kind: LoadErrorKind, host: string): Copy {
  switch (kind) {
    case 'dns_failure':
      return {
        title: `Can't find ${host}`,
        detail: 'The address could not be resolved. Check the spelling, or the site may no longer exist.',
      }
    case 'connection_refused':
      return {
        title: `Can't reach ${host}`,
        detail: 'The server refused the connection or is unreachable. It may be down or blocking access.',
      }
    case 'timeout':
      return {
        title: `${host} took too long to respond`,
        detail: 'The connection timed out. The site may be busy or your network may be slow.',
      }
    case 'tls_error':
      return {
        title: `Connection to ${host} is not secure`,
        detail: "The site's security certificate could not be verified, so Wis did not continue.",
      }
    case 'offline':
      return {
        title: 'You appear to be offline',
        detail: 'Wis could not reach the network. Check your connection and try again.',
      }
    default:
      return {
        title: `Can't open ${host}`,
        detail: 'The page failed to load.',
      }
  }
}

function hostOf(error: LoadError): string {
  try {
    return new URL(error.url).host || error.url
  } catch {
    return error.url
  }
}

interface NetErrorPageProps {
  tab: Tab
}

export function NetErrorPage({ tab }: NetErrorPageProps) {
  const [retrying, setRetrying] = useState(false)
  // The pane only renders this when load_error is set.
  const error = tab.load_error as LoadError
  const { title, detail } = copyFor(error.kind, hostOf(error))

  const retry = async () => {
    setRetrying(true)
    try {
      await commands.navigateTab(tab.id, error.url)
    } finally {
      setRetrying(false)
    }
  }

  return (
    <div className="kb-neterror-page">
      <div className="kb-neterror">
        <div className="kb-neterror-glyph" aria-hidden>
          <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.6} strokeLinecap="round" strokeLinejoin="round">
            <circle cx="12" cy="12" r="9" />
            <path d="M12 8v4" />
            <path d="M12 16h.01" />
          </svg>
        </div>
        <h1 className="kb-neterror-title">{title}</h1>
        <p className="kb-neterror-detail">{detail}</p>
        <p className="kb-neterror-url">{error.url}</p>
        <button
          type="button"
          className="kb-btn"
          onClick={() => void retry()}
          disabled={retrying}
        >
          {retrying ? 'Reloading…' : 'Try again'}
        </button>
      </div>
    </div>
  )
}
