import { useCallback, useEffect, useState } from 'react'

import { commands } from '@/bindings/commands'
import type { UaProfile, UserSettingsPatch } from '@/bindings/types'
import { Segmented } from '@/components/controls'
import { currentPlatform } from '@/lib/platform'

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

function defaultBrowserHelp(isDefault: boolean | null): string {
  if (isDefault === true) return 'Wis is your default browser. Links from other apps open here.'
  if (currentPlatform() === 'windows')
    return 'Registers Wis and opens Windows Settings, where you pick Wis as the default.'
  return 'Links clicked in other apps will open in Wis.'
}

interface CompatibilitySectionProps {
  profile: UaProfile
  overrides: Record<string, UaProfile>
  onUpdate: (patch: UserSettingsPatch) => void
}

export function CompatibilitySection({ profile, overrides, onUpdate }: CompatibilitySectionProps) {
  const [host, setHost] = useState('')
  const [newProfile, setNewProfile] = useState<SimpleUa>('chrome')
  const [isDefault, setIsDefault] = useState<boolean | null>(null)
  const [settingDefault, setSettingDefault] = useState(false)

  const refreshDefault = useCallback(() => {
    commands
      .defaultBrowserStatus()
      .then(setIsDefault)
      .catch(() => setIsDefault(null))
  }, [])

  useEffect(() => {
    refreshDefault()
    window.addEventListener('focus', refreshDefault)
    return () => window.removeEventListener('focus', refreshDefault)
  }, [refreshDefault])

  const setAsDefault = async () => {
    setSettingDefault(true)
    try {
      await commands.setDefaultBrowser()
      refreshDefault()
    } catch {
      // Leave the row as-is; the button stays available to retry.
    } finally {
      setSettingDefault(false)
    }
  }

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
          <span>Default browser</span>
          <span>{defaultBrowserHelp(isDefault)}</span>
        </div>
        <button
          type="button"
          className="kb-btn"
          onClick={() => void setAsDefault()}
          disabled={settingDefault || isDefault === true}
        >
          {isDefault === true ? 'Default' : 'Set as default'}
        </button>
      </div>
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
