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

/** Mirror of `wis_core::TlsFault::bypassable`. */
function bypassable(fault: TlsFault | null): boolean {
  return fault === 'expired' || fault === 'name_mismatch' || fault === 'untrusted'
}

interface TlsErrorPageProps {
  tab: Tab
}

/**
 * Full-page interstitial for TLS trust failures. The bypass is deliberately
 * buried behind "Advanced", offered only for recoverable certificate faults,
 * and scoped to this session.
 */
export function TlsErrorPage({ tab }: TlsErrorPageProps) {
  const [busy, setBusy] = useState<'retry' | 'proceed' | null>(null)
  const [advanced, setAdvanced] = useState(false)
  const error = tab.load_error as LoadError
  const host = hostOf(error)

  const retry = async () => {
    setBusy('retry')
    try {
      await commands.navigateTab(tab.id, error.url)
    } finally {
      setBusy(null)
    }
  }

  const proceed = async () => {
    setBusy('proceed')
    try {
      await commands.tlsBypassProceed(tab.id)
    } finally {
      setBusy(null)
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
        <div className="kb-tlserror-actions">
          <button
            type="button"
            className="kb-btn"
            onClick={() => void retry()}
            disabled={busy !== null}
          >
            {busy === 'retry' ? 'Reloading…' : 'Try again'}
          </button>
          {bypassable(error.tls) ? (
            <button
              type="button"
              className="kb-btn"
              aria-expanded={advanced}
              onClick={() => setAdvanced((cur) => !cur)}
            >
              Advanced
            </button>
          ) : null}
        </div>
        {advanced && bypassable(error.tls) ? (
          <div className="kb-tlserror-advanced">
            <p className="kb-tlserror-warning">
              Wis can&apos;t verify that this server is really {host}. Anything you send or
              receive could be read or altered by someone in the middle. This exception lasts
              until Wis quits.
            </p>
            <button
              type="button"
              className="kb-tlserror-proceed"
              onClick={() => void proceed()}
              disabled={busy !== null}
            >
              {busy === 'proceed' ? 'Connecting…' : `Proceed to ${host} (unsafe)`}
            </button>
          </div>
        ) : null}
      </div>
    </div>
  )
}
