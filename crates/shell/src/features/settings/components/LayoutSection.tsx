import type { LayoutMode, RailSide, UserSettings, UserSettingsPatch } from '@/bindings/types'
import { Segmented, Switch } from '@/components/controls'
import { PanelTopIcon, SidebarLeftIcon, SidebarRightIcon } from '@/lib/icons'

const LAYOUT_OPTIONS = [
  { value: 'sidebar_left' as const, label: 'Left', icon: <SidebarLeftIcon /> },
  { value: 'sidebar_right' as const, label: 'Right', icon: <SidebarRightIcon /> },
  { value: 'top_tabs' as const, label: 'Top', icon: <PanelTopIcon /> },
] satisfies ReadonlyArray<{ value: LayoutMode; label: string; icon: React.ReactNode }>

const RAIL_SIDE_OPTIONS = [
  { value: 'left' as const, label: 'Left', icon: <SidebarLeftIcon /> },
  { value: 'right' as const, label: 'Right', icon: <SidebarRightIcon /> },
] satisfies ReadonlyArray<{ value: RailSide; label: string; icon: React.ReactNode }>

/* Accordion limit for top-strip folder islands; '0' means no limit. */
const EXPANDED_FOLDER_LIMIT_OPTIONS = [
  { value: '1' as const, label: '1' },
  { value: '2' as const, label: '2' },
  { value: '3' as const, label: '3' },
  { value: '0' as const, label: 'No limit' },
] satisfies ReadonlyArray<{ value: string; label: string }>

interface LayoutSectionProps {
  settings: UserSettings
  onUpdate: (patch: UserSettingsPatch) => void
}

export function LayoutSection({ settings, onUpdate }: LayoutSectionProps) {
  return (
      <section id="layout" className="sec">
        <h2 className="sec-h">Layout</h2>
        <div className="row">
          <div className="l">
            <span>Tabs</span>
            <span>Where the tab strip lives</span>
          </div>
          <Segmented
            value={settings.layout}
            onChange={(layout) => {
              const patch: UserSettingsPatch = { layout }
              // Keep the rail side in sync with the active sidebar side.
              if (layout === 'sidebar_left') patch.top_tabs_rail_side = 'left'
              else if (layout === 'sidebar_right') patch.top_tabs_rail_side = 'right'
              onUpdate(patch)
            }}
            options={LAYOUT_OPTIONS}
            ariaLabel="Tab position"
          />
        </div>
        {settings.layout === 'top_tabs' && (
          <div className="row">
            <div className="l">
              <span>Rail side</span>
              <span>Which edge hosts the workspace + settings rail</span>
            </div>
            <Segmented
              value={settings.top_tabs_rail_side}
              onChange={(top_tabs_rail_side) =>
                onUpdate({ top_tabs_rail_side })
              }
              options={RAIL_SIDE_OPTIONS}
              ariaLabel="Rail side"
            />
          </div>
        )}
        {settings.layout === 'top_tabs' && (
          <div className="row">
            <div className="l">
              <span>Open folders</span>
              <span>Folders expanded at once; opening more collapses the oldest</span>
            </div>
            <Segmented
              value={String(settings.top_tabs_max_expanded_folders) as '0' | '1' | '2' | '3'}
              onChange={(v) =>
                onUpdate({ top_tabs_max_expanded_folders: Number(v) })
              }
              options={EXPANDED_FOLDER_LIMIT_OPTIONS}
              ariaLabel="Expanded folder limit"
            />
          </div>
        )}
        <div className="row">
          <div className="l">
            <span>Collapsible sidebar</span>
            <span>Expand on edge hover or click</span>
          </div>
          <Switch
            checked={settings.sidebar_collapsible}
            onChange={(sidebar_collapsible) => onUpdate({ sidebar_collapsible })}
            label="Collapsible sidebar"
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Video popout</span>
            <span>
              Switching away from a tab playing video floats it as an
              always-on-top mini player, free to leave the window. Click
              pauses, double-click left/right seeks, corners resize.
            </span>
          </div>
          <Switch
            checked={settings.video_popout_enabled}
            onChange={(video_popout_enabled) => onUpdate({ video_popout_enabled })}
            label="Video popout"
          />
        </div>
      </section>
  )
}
