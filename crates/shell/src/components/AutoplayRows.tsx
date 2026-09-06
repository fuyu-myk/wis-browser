/**
 * Settings -> Privacy: the autoplay policy and its per-site overrides.
 */

import { useState } from 'react'

import type { AutoplayPolicy, UserSettings, UserSettingsPatch } from '@/bindings/types'
import { Segmented } from '@/components/controls'
import { normalizeHostInput } from '@/lib/host'
import { currentPlatform } from '@/lib/platform'

const AUTOPLAY_OPTIONS = [
  { value: 'allow' as const, label: 'Allow' },
  { value: 'block_audible' as const, label: 'Block sound' },
  { value: 'block' as const, label: 'Block all' },
] satisfies ReadonlyArray<{ value: AutoplayPolicy; label: string }>

const PLATFORM = currentPlatform()

function policyLabel(policy: AutoplayPolicy): string {
  return AUTOPLAY_OPTIONS.find((o) => o.value === policy)?.label ?? policy
}

function policyHelp(): string {
  if (PLATFORM === 'windows')
    return 'Unavailable. Handled automatically on interaction by the engine.'
  if (PLATFORM === 'linux')
    return 'What media may start on its own. Block sound behaves as Block all.'
  return 'What media may start on its own. Block sound lets muted video play and holds sound until you interact. Applies on the next page load.'
}

interface AutoplayRowsProps {
  settings: UserSettings
  onUpdate: (patch: UserSettingsPatch) => void
}

export function AutoplayRows({ settings, onUpdate }: AutoplayRowsProps) {
  const [host, setHost] = useState('')
  const [newPolicy, setNewPolicy] = useState<AutoplayPolicy>('allow')
  const unavailable = PLATFORM === 'windows'
  const overrides = settings.autoplay_site_overrides
  const entries = Object.entries(overrides).sort(([a], [b]) => a.localeCompare(b))

  const addOverride = () => {
    const h = normalizeHostInput(host)
    if (h === '') return
    onUpdate({ autoplay_site_overrides: { ...overrides, [h]: newPolicy } })
    setHost('')
  }
  const removeOverride = (h: string) => {
    const next = Object.fromEntries(entries.filter(([key]) => key !== h))
    onUpdate({ autoplay_site_overrides: next })
  }

  return (
    <>
      <div className="row">
        <div className="l">
          <span>Autoplay</span>
          <span>{policyHelp()}</span>
        </div>
        {!unavailable && (
          <Segmented
            value={settings.autoplay_policy}
            onChange={(autoplay_policy) => onUpdate({ autoplay_policy })}
            options={AUTOPLAY_OPTIONS}
            ariaLabel="Autoplay policy"
          />
        )}
      </div>
      {!unavailable && (
        <>
          <div className="row">
            <div className="l">
              <span>Per-site autoplay</span>
              <span>Sites that get their own rule.</span>
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
              value={newPolicy}
              onChange={setNewPolicy}
              options={AUTOPLAY_OPTIONS}
              ariaLabel="Rule for the site"
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
                  <span className="kb-ua-prof">{policyLabel(p)}</span>
                  <button
                    type="button"
                    className="kb-ua-remove"
                    onClick={() => removeOverride(h)}
                    aria-label={`Remove autoplay rule for ${h}`}
                  >
                    Remove
                  </button>
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </>
  )
}
