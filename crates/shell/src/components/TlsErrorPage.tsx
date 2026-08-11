import { useState } from 'react'

import { commands } from '@/bindings/commands'
import type { LoadError, Tab, TlsFault } from '@/bindings/types'
import { hostOf } from '@/components/NetErrorPage'

function detailFor(fault: TlsFault | null, host: string): string {
  switch (fault) {
    case 'expired':
      return `The security certificate for ${host} is expired or not yet valid. This can happen when the site is misconfigured, or when your device's clock is wrong.`
    case 'name_mismatch':
      return `The security certificate for ${host} was issued for a different site. This can happen when a site is misconfigured, or when the connection is being intercepted.`
    case 'untrusted':
      return `The security certificate for ${host} is not from a trusted issuer. This can happen with self-signed certificates, or when the connection is being intercepted.`
    case 'revoked':
      return `The security certificate for ${host} has been revoked by its issuer, so Wis will not open this site.`
    default:
      return `A secure connection to ${host} could not be established.`
  }
}

interface TlsErrorPageProps {
  tab: Tab
}

/**
 * Full-page interstitial for TLS trust failures. Wis does not offer a
 * "proceed anyway" bypass: the privacy proxy owns trust evaluation and the
 * certificate challenge never reaches the webview to override.
 */
export function TlsErrorPage({ tab }: TlsErrorPageProps) {
  const [retrying, setRetrying] = useState(false)
  const error = tab.load_error as LoadError
  const host = hostOf(error)

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
        <div className="kb-neterror-glyph kb-tlserror-glyph" aria-hidden>
          <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.6} strokeLinecap="round" strokeLinejoin="round">
            <path d="M12 3 2.5 19.5h19L12 3z" />
            <path d="M12 10v4" />
            <path d="M12 17.5h.01" />
          </svg>
        </div>
        <h1 className="kb-neterror-title">Your connection to {host} isn&apos;t private</h1>
        <p className="kb-neterror-detail">{detailFor(error.tls, host)}</p>
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
