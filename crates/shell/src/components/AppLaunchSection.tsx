/**
 * Settings -> Privacy: remembered open-in-app decisions. Lists the
 * custom-scheme launches the user chose to persist (keyed
 * `"<scheme>|<initiator host>"` in `UserSettings.app_launch_permissions`);
 * each can be flipped allow<->block or removed.
 */

import { cx } from '@/lib/cx'
import { XIcon } from '@/lib/icons'

interface AppLaunchSectionProps {
  permissions: Record<string, boolean>
  onChange: (next: Record<string, boolean>) => void
}

export function AppLaunchSection({ permissions, onChange }: AppLaunchSectionProps) {
  const entries = Object.keys(permissions)
    .map((key) => {
      const sep = key.indexOf('|')
      return {
        key,
        scheme: key.slice(0, sep),
        host: key.slice(sep + 1),
        granted: permissions[key],
      }
    })
    .sort((a, b) => a.host.localeCompare(b.host) || a.scheme.localeCompare(b.scheme))

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
          <span>Open in app</span>
          <span>
            Sites remembered as allowed or blocked from opening links in other
            apps. Remove one to be asked again the next time that site tries.
          </span>
        </div>
      </div>
      {entries.length === 0 ? (
        <p className="kb-filterlists-status">No remembered open-in-app decisions.</p>
      ) : (
        <ul className="kb-taskman-list">
          {entries.map(({ key, scheme, host, granted }) => (
            <li className="kb-perm-row" key={key}>
              <span className="kb-perm-row-origin" title={host || 'Any site'}>
                {host || 'Any site'}
              </span>
              <span className="kb-perm-row-device">{scheme}</span>
              <button
                type="button"
                className={cx('kb-perm-state', { 'is-blocked': !granted })}
                onClick={() => flip(key)}
                aria-label={
                  granted
                    ? `Allowed; click to block ${scheme} links from ${host}`
                    : `Blocked; click to allow ${scheme} links from ${host}`
                }
              >
                {granted ? 'Allowed' : 'Blocked'}
              </button>
              <button
                type="button"
                className="kb-taskman-x"
                aria-label={`Remove ${scheme} for ${host}`}
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
