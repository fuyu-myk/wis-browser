/**
 * Settings -> Privacy: user scripts (Tampermonkey-style). Import a `.user.js`
 * file (native open panel), toggle each on/off, and remove. Scripts run in
 * the page world on URLs matching their `@match`/`@include` patterns; GM_*
 * APIs are not provided (see `wis_core::userscript`).
 */

import { useState } from 'react'

import { commands } from '@/bindings/commands'
import type { Userscript } from '@/bindings/types'
import { XIcon } from '@/lib/icons'
import { Switch } from '@/components/controls'

interface UserscriptsSectionProps {
  scripts: Userscript[]
  onChange: (next: Userscript[]) => void
}

export function UserscriptsSection({ scripts, onChange }: UserscriptsSectionProps) {
  const [error, setError] = useState<string | null>(null)

  const importOne = () => {
    setError(null)
    void commands
      .importUserscript()
      .then((script) => {
        if (!script) return
        // Replace by id (re-import) or append.
        const without = scripts.filter((s) => s.id !== script.id)
        onChange([...without, script])
      })
      .catch((err: unknown) => {
        setError(err instanceof Error ? err.message : String(err))
      })
  }

  const setEnabled = (id: string, enabled: boolean) => {
    onChange(scripts.map((s) => (s.id === id ? { ...s, enabled } : s)))
  }
  const remove = (id: string) => {
    onChange(scripts.filter((s) => s.id !== id))
  }

  return (
    <>
      <div className="row">
        <div className="l">
          <span>User scripts</span>
          <span>
            Run your own JavaScript on matching sites. Scripts run in the page
            (no GM_* APIs); only add ones you trust.
          </span>
        </div>
        <button type="button" className="kb-btn" onClick={importOne}>
          Import .user.js
        </button>
      </div>
      {error !== null && <p className="kb-filterlists-status" role="alert">Import failed: {error}</p>}
      {scripts.length > 0 && (
        <ul className="kb-taskman-list">
          {scripts.map((script) => (
            <li className="kb-taskman-row" key={script.id}>
              <Switch
                checked={script.enabled}
                onChange={(enabled) => setEnabled(script.id, enabled)}
                label={`Enable ${script.name}`}
              />
              <span className="kb-taskman-name" title={script.matches.join(', ') || 'no matches'}>
                {script.name}
              </span>
              <span className="kb-filterlists-detail">
                {script.matches.length} match{script.matches.length === 1 ? '' : 'es'}
                {' · '}
                {script.run_at.replace('document-', '')}
              </span>
              <button
                type="button"
                className="kb-taskman-x"
                aria-label={`Remove ${script.name}`}
                title="Remove"
                onClick={() => remove(script.id)}
              >
                <XIcon />
              </button>
            </li>
          ))}
        </ul>
      )}
    </>
  )
}
