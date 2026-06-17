import type { ThemeMode, UserSettings, UserSettingsPatch, Workspace } from '@/bindings/types'
import { Segmented, Switch } from '@/components/controls'
import { BackgroundEditor } from '@/components/BackgroundEditor'
import { MonitorIcon, MoonIcon, SunIcon } from '@/lib/icons'

const THEME_OPTIONS = [
  { value: 'light' as const, label: 'Light', icon: <SunIcon /> },
  { value: 'dark' as const, label: 'Dark', icon: <MoonIcon /> },
  { value: 'system' as const, label: 'Auto', icon: <MonitorIcon /> },
] satisfies ReadonlyArray<{ value: ThemeMode; label: string; icon: React.ReactNode }>

interface AppearanceSectionProps {
  settings: UserSettings
  activeWorkspace: Workspace | undefined
  onUpdate: (patch: UserSettingsPatch) => void
}

export function AppearanceSection({ settings, activeWorkspace, onUpdate }: AppearanceSectionProps) {
  return (
      <section id="appearance" className="sec">
        <div className="sec-h">Appearance</div>
        <div className="row">
          <div className="l">
            <span>Theme</span>
            <span>Light, dark, or follow system</span>
          </div>
          <Segmented
            value={settings.theme}
            onChange={(theme) => onUpdate({ theme })}
            options={THEME_OPTIONS}
            ariaLabel="Theme"
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Motion</span>
            <span>
              Reduced keeps the same timings without the springiness; Off
              disables non-essential animation. Follows the OS reduce-motion
              preference when set to Full.
            </span>
          </div>
          <Segmented
            value={settings.motion}
            onChange={(motion) => onUpdate({ motion })}
            options={[
              { value: 'full' as const, label: 'Full' },
              { value: 'reduced' as const, label: 'Reduced' },
              { value: 'off' as const, label: 'Off' },
            ]}
            ariaLabel="Animation level"
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Liquid glass</span>
            <span>Blur and translucency</span>
          </div>
          <Switch
            checked={settings.glass_enabled}
            onChange={(glass_enabled) => onUpdate({ glass_enabled })}
            label="Liquid glass"
          />
        </div>
        <div className="row">
          <div className="l">
            <span>URL bar focus glow</span>
            <span>Outline the address bar in the accent colour when focused</span>
          </div>
          <Switch
            checked={settings.url_bar_focus_glow}
            onChange={(url_bar_focus_glow) => onUpdate({ url_bar_focus_glow })}
            label="URL bar focus glow"
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Workspace background</span>
            <span>
              Applies to the current workspace
              {activeWorkspace ? ` (${activeWorkspace.name})` : ''}. Gradients,
              images and GIF wallpapers can be set.
            </span>
          </div>
        </div>
        {activeWorkspace && !activeWorkspace.ephemeral && (
          <BackgroundEditor ws={activeWorkspace} />
        )}
      </section>
  )
}
