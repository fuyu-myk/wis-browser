/**
 * Settings -> Privacy: remembered camera/microphone site permissions. Lists the
 * decisions the user chose to persist (keyed `"<kind>|<origin>"` in
 * `UserSettings.media_permissions`); each can be flipped allow<->block or
 * removed (removing makes the site ask again next time).
 */

import { cx } from '@/lib/cx'
import { XIcon } from '@/lib/icons'
import { deviceLabel } from '@/features/site/lib/siteRules'

interface SitePermissionsSectionProps {
  permissions: Record<string, boolean>
  onChange: (next: Record<string, boolean>) => void
}

/** Authority (host[:port]) of `scheme://host[:port]/...`. */
function hostOf(origin: string): string {
  const rest = origin.includes('://') ? origin.split('://')[1] : origin
  return rest.split('/')[0] || origin
}

export function SitePermissionsSection({ permissions, onChange }: SitePermissionsSectionProps) {
  const entries = Object.keys(permissions)
    .map((key) => {
      const sep = key.indexOf('|')
      return {
        key,
        kind: key.slice(0, sep),
        origin: key.slice(sep + 1),
        granted: permissions[key],
      }
    })
    .sort((a, b) => a.origin.localeCompare(b.origin) || a.kind.localeCompare(b.kind))

  const flip = (key: string) => onChange({ ...permissions, [key]: !permissions[key] })
  const remove = (key: string) => {
    const next = { ...permissions }
    delete next[key]
    onChange(next)
  }

  return (
    <>
      <div className="row">
        <div className="l">
          <span>Site permissions</span>
          <span>
            Camera and microphone decisions you chose to remember. Remove one to
            be asked again the next time that site requests it.
          </span>
        </div>
      </div>
      {entries.length === 0 ? (
        <p className="kb-filterlists-status">No remembered camera or microphone permissions.</p>
      ) : (
        <ul className="kb-taskman-list">
          {entries.map(({ key, kind, origin, granted }) => (
            <li className="kb-perm-row" key={key}>
              <span className="kb-perm-row-origin" title={origin}>
                {hostOf(origin)}
              </span>
              <span className="kb-perm-row-device">{deviceLabel(kind)}</span>
              <button
                type="button"
                className={cx('kb-perm-state', { 'is-blocked': !granted })}
                onClick={() => flip(key)}
                aria-label={
                  granted
                    ? `Allowed; click to block ${origin}`
                    : `Blocked; click to allow ${origin}`
                }
              >
                {granted ? 'Allowed' : 'Blocked'}
              </button>
              <button
                type="button"
                className="kb-taskman-x"
                aria-label={`Remove ${origin}`}
                title="Remove (the site will ask again)"
                onClick={() => remove(key)}
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
