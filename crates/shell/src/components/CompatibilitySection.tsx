import { useState } from 'react'

import type { UaProfile, UserSettingsPatch } from '@/bindings/types'
import { Segmented } from '@/components/controls'

type SimpleUa = 'safari' | 'chrome' | 'default'

const UA_OPTIONS = [
  { value: 'safari' as const, label: 'Safari' },
  { value: 'chrome' as const, label: 'Chrome' },
  { value: 'default' as const, label: 'Engine' },
] satisfies ReadonlyArray<{ value: SimpleUa; label: string }>

function uaProfileLabel(p: UaProfile): string {
  if (p === 'safari') return 'Safari'
  if (p === 'chrome') return 'Chrome'
  if (p === 'default') return 'Engine default'
  return 'Custom'
}

/** Reduce a pasted URL or `host/path` to a `www.`-stripped, lowercased host. */
function normalizeHostInput(raw: string): string {
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

interface CompatibilitySectionProps {
  profile: UaProfile
  overrides: Record<string, UaProfile>
  onUpdate: (patch: UserSettingsPatch) => void
}

export function CompatibilitySection({ profile, overrides, onUpdate }: CompatibilitySectionProps) {
  const [host, setHost] = useState('')
  const [newProfile, setNewProfile] = useState<SimpleUa>('chrome')

  const globalValue: SimpleUa = typeof profile === 'string' ? profile : 'safari'
  const entries = Object.entries(overrides).sort(([a], [b]) => a.localeCompare(b))

  const addOverride = () => {
    const h = normalizeHostInput(host)
    if (!h) return
    onUpdate({ ua_site_overrides: { ...overrides, [h]: newProfile } })
    setHost('')
  }

  const removeOverride = (h: string) => {
    const next = { ...overrides }
    delete next[h]
    onUpdate({ ua_site_overrides: next })
  }

  return (
    <section id="compatibility" className="sec">
      <div className="sec-h">Compatibility</div>
      <div className="row">
        <div className="l">
          <span>Browser identity</span>
          <span>
            How Wis identifies to sites. Safari is recommended (the engine is
            WebKit). Switch a stubborn site to Chrome below if it still reports
            an unsupported browser.
          </span>
        </div>
        <Segmented
          value={globalValue}
          onChange={(v) => onUpdate({ user_agent_profile: v })}
          options={UA_OPTIONS}
          ariaLabel="Default browser identity"
        />
      </div>
      <div className="row">
        <div className="l">
          <span>Per-site identity</span>
          <span>Override the identity for specific sites that need it.</span>
        </div>
      </div>
      <div className="kb-ua-add">
        <input
          type="text"
          className="kb-text-input"
          placeholder="example.com"
          value={host}
          onChange={(e) => setHost(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') addOverride()
          }}
          aria-label="Site host"
        />
        <Segmented
          value={newProfile}
          onChange={(v) => setNewProfile(v)}
          options={UA_OPTIONS}
          ariaLabel="Override identity"
        />
        <button
          type="button"
          className="kb-btn"
          onClick={addOverride}
          disabled={normalizeHostInput(host) === ''}
        >
          Add
        </button>
      </div>
      {entries.length > 0 && (
        <ul className="kb-ua-list">
          {entries.map(([h, p]) => (
            <li key={h} className="kb-ua-item">
              <span className="kb-ua-host">{h}</span>
              <span className="kb-ua-prof">{uaProfileLabel(p)}</span>
              <button
                type="button"
                className="kb-ua-remove"
                onClick={() => removeOverride(h)}
                aria-label={`Remove override for ${h}`}
              >
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}
