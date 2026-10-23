/**
 * Leading URL-bar icon that reflects the active tab's connection security:
 * a padlock for HTTPS, a device mark for plain HTTP to this machine, a "not
 * secure" mark for other plain HTTP, the Wis mark for Wis's own pages, and the
 * neutral search glyph for the launchpad. On a website it opens the site panel.
 */

import { useRef, type MouseEvent, type ReactNode } from 'react'

import type { Tab } from '@/bindings/types'
import { WisMark } from '@/components/WisMark'
import { useBrowserState } from '@/hooks/useBrowserState'
import { SITE_BUTTON_ATTR, useSitePanel } from '@/features/site/hooks/useSitePanel'
import { useSiteProtection } from '@/features/site/hooks/useSiteProtection'
import { cx } from '@/lib/cx'
import { SearchIcon } from '@/lib/icons'
import { applicableProtection, fingerprintingApplies, isUnprotected, siteOf } from '@/features/site/lib/sitePanel'
import { connectionKind, NEW_TAB_URL } from '@/lib/url'

const CONNECTIONS: Record<'secure' | 'local' | 'insecure', { className: string; title: string; label: string; glyph: ReactNode }> = {
  secure: {
    className: 'kb-secure',
    title: 'Connection is secure (HTTPS)',
    label: 'Secure connection',
    glyph: <LockGlyph />,
  },
  local: {
    className: 'kb-local',
    title: 'Local address: served from this device',
    label: 'Local address',
    glyph: <LocalGlyph />,
  },
  insecure: {
    className: 'kb-insecure',
    title: 'Not secure: this site is loaded over HTTP',
    label: 'Connection is not secure',
    glyph: <NotSecureGlyph />,
  },
}

const keepFocus = (e: MouseEvent) => {
  e.preventDefault()
  e.stopPropagation()
}

export function SiteButton({ activeTab }: { activeTab: Tab | null }) {
  const buttonRef = useRef<HTMLButtonElement>(null)
  const panel = useSitePanel(buttonRef, activeTab)
  const url = activeTab?.url ?? ''
  const site = siteOf(url)
  const { data: state } = useBrowserState()
  const mode = state?.workspaces.find((w) => w.id === activeTab?.workspace_id)?.privacy_mode
  const unprotected = isUnprotected(applicableProtection(useSiteProtection(site), fingerprintingApplies(mode)))

  if (!activeTab || !url || url === NEW_TAB_URL) return <SearchIcon className="ico" />
  const kind = connectionKind(url)
  if (kind === 'wis') {
    return (
      <span className="ico kb-chrome-page" title="Wis page" aria-label="Wis page">
        <WisMark variant="solid-closed" size={14} />
      </span>
    )
  }

  const connection = CONNECTIONS[kind]
  if (!site) {
    return (
      <span className={cx('ico', connection.className)} title={connection.title} aria-label={connection.label}>
        {connection.glyph}
      </span>
    )
  }

  return (
    <button
      ref={buttonRef}
      type="button"
      className={cx('ico kb-site-button', connection.className, { 'is-unprotected': unprotected })}
      title={unprotected ? `Protection is off for ${site}` : connection.title}
      aria-label={`${connection.label}. Settings for ${site}`}
      {...{ [SITE_BUTTON_ATTR]: '' }}
      onMouseDown={keepFocus}
      onClick={() => panel.toggle(activeTab)}
    >
      {connection.glyph}
    </button>
  )
}

function LockGlyph() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <rect x="5" y="11" width="14" height="9" rx="2" />
      <path d="M8 11V8a4 4 0 0 1 8 0v3" />
    </svg>
  )
}

function LocalGlyph() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <rect x="4" y="5" width="16" height="11" rx="2" />
      <path d="M2 20h20" />
    </svg>
  )
}

function NotSecureGlyph() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M10.3 3.8 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.8a2 2 0 0 0-3.4 0Z" />
      <path d="M12 9v4" />
      <path d="M12 17h.01" />
    </svg>
  )
}
