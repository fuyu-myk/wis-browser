/**
 * Settings -> Appearance: the default page zoom and the sites that have their
 * own.
 */

import { commands } from '@/bindings/commands'
import type { UserSettings, UserSettingsPatch } from '@/bindings/types'
import { ZOOM_LADDER, saneZoomPercent } from '@/features/zoom/zoom'

interface ZoomRowsProps {
  settings: UserSettings
  onUpdate: (patch: UserSettingsPatch) => void
}

export function ZoomRows({ settings, onUpdate }: ZoomRowsProps) {
  const overrides = settings.zoom_site_overrides
  const entries = Object.entries(overrides).sort(([a], [b]) => a.localeCompare(b))

  const forget = (host: string) => {
    void commands.zoomForgetSite(host).catch(() => undefined)
  }

  return (
    <>
      <div className="row">
        <div className="l">
          <span>Default zoom</span>
          <span>How pages are sized unless a site has its own zoom.</span>
        </div>
        <span className="kb-select">
          <select
            value={saneZoomPercent(settings.default_zoom_percent)}
            onChange={(e) => onUpdate({ default_zoom_percent: Number(e.currentTarget.value) })}
            aria-label="Default zoom"
          >
            {ZOOM_LADDER.map((percent) => (
              <option key={percent} value={percent}>
                {percent}%
              </option>
            ))}
          </select>
        </span>
      </div>
      <div className="row">
        <div className="l">
          <span>Sites with their own zoom</span>
          <span>
            {entries.length === 0
              ? 'Zoom a page and it is stored here for that site.'
              : 'Set by zooming a page. Resetting a page zoom removes it.'}
          </span>
        </div>
      </div>
      {entries.length > 0 && (
        <ul className="kb-ua-list">
          {entries.map(([host, percent]) => (
            <li key={host} className="kb-ua-item">
              <span className="kb-ua-host">{host}</span>
              <span className="kb-ua-prof">{percent}%</span>
              <button
                type="button"
                className="kb-ua-remove"
                onClick={() => forget(host)}
                aria-label={`Forget the zoom for ${host}`}
              >
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}
    </>
  )
}
