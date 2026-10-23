import type { UserSettings, UserSettingsPatch } from '@/bindings/types'
import { Switch } from '@/components/controls'

interface DownloadsSectionProps {
  settings: UserSettings
  onUpdate: (patch: UserSettingsPatch) => void
}

export function DownloadsSection({ settings, onUpdate }: DownloadsSectionProps) {
  return (
      <section id="downloads" className="sec">
        <div className="sec-h">Downloads</div>
        <div className="row">
          <div className="l">
            <span>Ask where to save each download</span>
            <span>
              Choose a destination in a save panel instead of saving straight
              to Downloads. The panel starts in the last folder you picked
              {settings.download_last_dir ? ` (${settings.download_last_dir})` : ''}.
            </span>
          </div>
          <Switch
            checked={settings.download_ask_where}
            onChange={(download_ask_where) => onUpdate({ download_ask_where })}
            label="Ask where to save each download"
          />
        </div>
      </section>
  )
}
