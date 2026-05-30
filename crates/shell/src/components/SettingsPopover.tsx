import { useEffect, useRef } from 'react'

import { commands } from '@/bindings/commands'
import type {
  AccentColor,
  LayoutMode,
  PrivacyMode,
  RailSide,
  ThemeMode,
  UrlBarPosition,
  UserSettings,
  UserSettingsPatch,
  WorkspaceId,
} from '@/bindings/types'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { BROWSER_STATE_KEY, useBrowserState, useUpdateSettings } from '@/hooks/useBrowserState'
import {
  MonitorIcon,
  MoonIcon,
  PanelTopIcon,
  SidebarLeftIcon,
  SidebarRightIcon,
  SunIcon,
  XIcon,
} from '@/lib/icons'

import { Segmented, SwatchRow, Switch } from '@/components/controls'

const THEME_OPTIONS = [
  { value: 'light' as const, label: 'Light', icon: <SunIcon /> },
  { value: 'dark' as const, label: 'Dark', icon: <MoonIcon /> },
  { value: 'system' as const, label: 'Auto', icon: <MonitorIcon /> },
] satisfies ReadonlyArray<{ value: ThemeMode; label: string; icon: React.ReactNode }>

const LAYOUT_OPTIONS = [
  { value: 'sidebar_left' as const, label: 'Left', icon: <SidebarLeftIcon /> },
  { value: 'sidebar_right' as const, label: 'Right', icon: <SidebarRightIcon /> },
  { value: 'top_tabs' as const, label: 'Top', icon: <PanelTopIcon /> },
] satisfies ReadonlyArray<{ value: LayoutMode; label: string; icon: React.ReactNode }>

const RAIL_SIDE_OPTIONS = [
  { value: 'left' as const, label: 'Left', icon: <SidebarLeftIcon /> },
  { value: 'right' as const, label: 'Right', icon: <SidebarRightIcon /> },
] satisfies ReadonlyArray<{ value: RailSide; label: string; icon: React.ReactNode }>

const URL_BAR_OPTIONS = [
  { value: 'sidebar' as const, label: 'Side' },
  { value: 'top' as const, label: 'Top' },
] satisfies ReadonlyArray<{ value: UrlBarPosition; label: string }>

const ACCENT_OPTIONS = [
  { value: 'primary' as const, swatchClass: 'fav-pri', label: 'Periwinkle' },
  { value: 'mint' as const, swatchClass: 'fav-mint', label: 'Mint' },
  { value: 'rose' as const, swatchClass: 'fav-rose', label: 'Rose' },
  { value: 'sun' as const, swatchClass: 'fav-sun', label: 'Sun' },
  { value: 'lila' as const, swatchClass: 'fav-lila', label: 'Lila' },
  { value: 'azur' as const, swatchClass: 'fav-azur', label: 'Azur' },
  { value: 'gray' as const, swatchClass: 'fav-gray', label: 'Gray' },
  { value: 'ink' as const, swatchClass: 'fav-ink', label: 'Ink' },
] satisfies ReadonlyArray<{ value: AccentColor; swatchClass: string; label: string }>

const PRIVACY_OPTIONS = [
  { value: 'strict' as const, label: 'Strict' },
  { value: 'balanced' as const, label: 'Balanced' },
  { value: 'permissive' as const, label: 'Permissive' },
] satisfies ReadonlyArray<{ value: PrivacyMode; label: string }>

const PRIVACY_DESCRIPTIONS: Record<PrivacyMode, string> = {
  strict: 'HTTPS-only, all third-party and trackers blocked, DoH required',
  balanced: 'HTTPS-first, third-party cookies and trackers blocked',
  permissive: 'Minimal automatic blocking: site-by-site control',
}

interface SettingsPopoverProps {
  settings: UserSettings
  onClose: () => void
}

export function SettingsPopover({ settings, onClose }: SettingsPopoverProps) {
  const ref = useRef<HTMLDivElement>(null)
  const update = useUpdateSettings()
  const qc = useQueryClient()
  const { data: state } = useBrowserState()
  const activeWorkspace = state?.workspaces.find((w) => w.id === state.active_workspace_id)
  const setWorkspaceMode = useMutation({
    mutationFn: ({ id, mode }: { id: WorkspaceId; mode: PrivacyMode }) =>
      commands.setWorkspacePrivacyMode(id, mode),
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  return (
    <div
      className="kb-settings-backdrop"
      role="presentation"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
    <div
      ref={ref}
      className="kb-settings-pop is-modal"
      role="dialog"
      aria-modal="true"
      aria-label="Settings"
    >
      <div className="kb-settings-pop-header">
        <span className="kb-settings-pop-title">Tweaks</span>
        <button
          type="button"
          className="kb-iconbtn kb-settings-close"
          aria-label="Close settings"
          onClick={onClose}
        >
          <XIcon />
        </button>
      </div>

      <div className="sec">
        <div className="sec-h">Appearance</div>
        <div className="row">
          <div className="l">
            <span>Theme</span>
            <span>Light, dark, or follow system</span>
          </div>
          <Segmented
            value={settings.theme}
            onChange={(theme) => update.mutate({ theme })}
            options={THEME_OPTIONS}
            ariaLabel="Theme"
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Accent</span>
            <span>Recolours the chrome highlights</span>
          </div>
        </div>
        <SwatchRow
          value={settings.accent}
          onChange={(accent) => update.mutate({ accent })}
          options={ACCENT_OPTIONS}
          ariaLabel="Accent colour"
        />
        <div className="row">
          <div className="l">
            <span>Liquid glass</span>
            <span>Blur and translucency</span>
          </div>
          <Switch
            checked={settings.glass_enabled}
            onChange={(glass_enabled) => update.mutate({ glass_enabled })}
            label="Liquid glass"
          />
        </div>
      </div>

      <div className="sec">
        <div className="sec-h">Layout</div>
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
              update.mutate(patch)
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
                update.mutate({ top_tabs_rail_side })
              }
              options={RAIL_SIDE_OPTIONS}
              ariaLabel="Rail side"
            />
          </div>
        )}
        <div className="row">
          <div className="l">
            <span>Address bar</span>
            <span>Inline in sidebar, or pinned at the top</span>
          </div>
          <Segmented
            value={settings.url_bar_position}
            onChange={(url_bar_position) => update.mutate({ url_bar_position })}
            options={URL_BAR_OPTIONS}
            ariaLabel="Address bar position"
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Collapsible sidebar</span>
            <span>Expand on edge hover or click</span>
          </div>
          <Switch
            checked={settings.sidebar_collapsible}
            onChange={(sidebar_collapsible) => update.mutate({ sidebar_collapsible })}
            label="Collapsible sidebar"
          />
        </div>
      </div>

      <div className="sec">
        <div className="sec-h">Privacy</div>
        {activeWorkspace ? (
          <div className="row">
            <div className="l">
              <span>This workspace ({activeWorkspace.name})</span>
              <span>{PRIVACY_DESCRIPTIONS[activeWorkspace.privacy_mode]}</span>
            </div>
            <Segmented
              value={activeWorkspace.privacy_mode}
              onChange={(mode) =>
                setWorkspaceMode.mutate({ id: activeWorkspace.id, mode })
              }
              options={PRIVACY_OPTIONS}
              ariaLabel="Workspace privacy mode"
            />
          </div>
        ) : null}
        <div className="row">
          <div className="l">
            <span>Default for new workspaces</span>
            <span>{PRIVACY_DESCRIPTIONS[settings.default_privacy_mode]}</span>
          </div>
          <Segmented
            value={settings.default_privacy_mode}
            onChange={(default_privacy_mode) => update.mutate({ default_privacy_mode })}
            options={PRIVACY_OPTIONS}
            ariaLabel="Default privacy mode"
          />
        </div>
      </div>
    </div>
    </div>
  )
}
