/**
 * Settings -> Privacy: sites with blocking or JavaScript switched off from the
 * padlock's panel. Removing one turns both back on.
 */

import type { SiteProtection } from '@/bindings/types'
import { XIcon } from '@/lib/icons'
import { protectionLabel } from '@/lib/sitePanel'

interface SiteProtectionSectionProps {
  overrides: Record<string, SiteProtection>
  onChange: (next: Record<string, SiteProtection>) => void
}

export function SiteProtectionSection({ overrides, onChange }: SiteProtectionSectionProps) {
  const entries = Object.entries(overrides).sort(([a], [b]) => a.localeCompare(b))

  const remove = (host: string) => onChange(Object.fromEntries(entries.filter(([h]) => h !== host)))

  return (
    <>
      <div className="row">
        <div className="l">
          <span>Site protection</span>
          <span>Sites where you turned blocking or JavaScript off from the padlock. Remove one to turn both back on.</span>
        </div>
      </div>
      {entries.length === 0 ? (
        <p className="kb-filterlists-status">No sites with protection turned off.</p>
      ) : (
        <ul className="kb-taskman-list">
          {entries.map(([host, protection]) => (
            <li className="kb-perm-row" key={host}>
              <span className="kb-perm-row-origin" title={host}>
                {host}
              </span>
              <span className="kb-perm-row-device">{protectionLabel(protection)}</span>
              <button
                type="button"
                className="kb-taskman-x"
                aria-label={`Turn protection back on for ${host}`}
                title="Remove (protection turns back on)"
                onClick={() => remove(host)}
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
