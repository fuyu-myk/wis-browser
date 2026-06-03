import { useEffect, useRef, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type {
  AccentColor,
  LayoutMode,
  PrivacyMode,
  RailSide,
  ThemeMode,
  UrlBarPosition,
  UserSettingsPatch,
  WorkspaceId,
} from '@/bindings/types'
import { cx } from '@/lib/cx'
import { BROWSER_STATE_KEY, useBrowserState, useUpdateSettings } from '@/hooks/useBrowserState'
import {
  LockIcon,
  MonitorIcon,
  MoonIcon,
  PanelTopIcon,
  SidebarLeftIcon,
  SidebarRightIcon,
  SunIcon,
} from '@/lib/icons'
import { DEFAULT_KEYMAP, actionLabel, allActions, effectiveKeymap } from '@/lib/palette'
import type { KeyAction } from '@/bindings/types'

import { Segmented, SwatchRow, Switch } from '@/components/controls'
import { KeybindingInput } from '@/components/KeybindingInput'

interface NavSection {
  id: string
  label: string
  icon: React.ReactNode
}

const NAV_SECTIONS: ReadonlyArray<NavSection> = [
  { id: 'appearance', label: 'Appearance', icon: <SunIcon /> },
  { id: 'layout', label: 'Layout', icon: <PanelTopIcon /> },
  { id: 'privacy', label: 'Privacy', icon: <LockIcon /> },
  { id: 'shortcuts', label: 'Shortcuts', icon: <KeyboardIcon /> },
]

function KeyboardIcon() {
  return (
    <svg
      width={16}
      height={16}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <rect x="2" y="6" width="20" height="12" rx="2" />
      <path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10" />
    </svg>
  )
}

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

export function SettingsPage() {
  const { data: state } = useBrowserState()
  const update = useUpdateSettings()
  const qc = useQueryClient()
  const activeWorkspace = state?.workspaces.find((w) => w.id === state.active_workspace_id)
  const setWorkspaceMode = useMutation({
    mutationFn: ({ id, mode }: { id: WorkspaceId; mode: PrivacyMode }) =>
      commands.setWorkspacePrivacyMode(id, mode),
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })
  const [refreshStatus, setRefreshStatus] = useState<string | null>(null)
  const refreshLists = useMutation({
    mutationFn: () => commands.refreshFilterLists(),
    onSuccess: (size) => {
      setRefreshStatus(`Updated. ${formatBytes(size)} of rules now active.`)
    },
    onError: (err: unknown) => {
      const msg = err instanceof Error ? err.message : String(err)
      setRefreshStatus(`Refresh failed: ${msg}`)
    },
  })

  const mainRef = useRef<HTMLDivElement | null>(null)
  const [activeSection, setActiveSection] = useState<string>(NAV_SECTIONS[0].id)

  // Highlight the section currently dominating the viewport.
  useEffect(() => {
    const root = mainRef.current
    if (!root) return
    const observed = NAV_SECTIONS.map((s) => root.querySelector<HTMLElement>(`#${s.id}`)).filter(
      (el): el is HTMLElement => el !== null,
    )
    if (observed.length === 0) return
    const obs = new IntersectionObserver(
      (entries) => {
        const visible = entries
          .filter((e) => e.isIntersecting)
          .sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top)
        if (visible.length > 0 && visible[0].target.id) {
          setActiveSection(visible[0].target.id)
        }
      },
      { root, threshold: 0, rootMargin: '0px 0px -70% 0px' },
    )
    for (const el of observed) obs.observe(el)
    return () => obs.disconnect()
  }, [])

  if (!state) return null
  const settings = state.settings
  const navOnRight = settings.layout === 'sidebar_right'

  const scrollTo = (id: string) => {
    const el = mainRef.current?.querySelector(`#${id}`)
    if (el) el.scrollIntoView({ behavior: 'smooth', block: 'start' })
  }

  return (
    <div
      className={cx('kb-settings-page', { 'nav-right': navOnRight })}
      aria-label="Settings"
    >
      <aside className="kb-settings-nav" aria-label="Settings sections">
        <div className="kb-settings-nav-title">Settings</div>
        {NAV_SECTIONS.map((s) => (
          <button
            key={s.id}
            type="button"
            className={cx({ 'is-active': activeSection === s.id })}
            onClick={() => scrollTo(s.id)}
          >
            {s.icon}
            <span>{s.label}</span>
          </button>
        ))}
      </aside>
      <div className="kb-settings-main" ref={mainRef}>
      <section id="appearance" className="sec">
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
        <div className="row">
          <div className="l">
            <span>URL bar focus glow</span>
            <span>Outline the address bar in the accent colour when focused</span>
          </div>
          <Switch
            checked={settings.url_bar_focus_glow}
            onChange={(url_bar_focus_glow) => update.mutate({ url_bar_focus_glow })}
            label="URL bar focus glow"
          />
        </div>
      </section>

      <section id="layout" className="sec">
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
      </section>

      <section id="privacy" className="sec">
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
        <div className="row">
          <div className="l">
            <span>Auto-update filter lists</span>
            <span>Fetch EasyPrivacy on launch, cache for 24 hours.</span>
          </div>
          <Switch
            checked={settings.filter_list_auto_update}
            onChange={(filter_list_auto_update) =>
              update.mutate({ filter_list_auto_update })
            }
            label="Auto-update filter lists"
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Refresh now</span>
            <span>
              {refreshStatus ?? 'Download the latest filter list and apply to open tabs.'}
            </span>
          </div>
          <button
            type="button"
            className="kb-btn"
            onClick={() => {
              setRefreshStatus(null)
              refreshLists.mutate()
            }}
            disabled={refreshLists.isPending}
          >
            {refreshLists.isPending ? 'Refreshing.' : 'Refresh'}
          </button>
        </div>
      </section>

      <KeyboardShortcutsSection
        keybindings={settings.keybindings}
        onChange={(patch) => update.mutate({ keybindings: patch })}
      />
      </div>
    </div>
  )
}

interface KeyboardShortcutsSectionProps {
  keybindings: Partial<Record<KeyAction, string>>
  onChange: (patch: Partial<Record<KeyAction, string>>) => void
}

function KeyboardShortcutsSection({
  keybindings,
  onChange,
}: KeyboardShortcutsSectionProps) {
  const effective = effectiveKeymap(keybindings)
  const accelToActions = new Map<string, KeyAction[]>()
  for (const action of allActions()) {
    const accel = effective[action]
    if (!accel) continue
    const list = accelToActions.get(accel) ?? []
    list.push(action)
    accelToActions.set(accel, list)
  }

  return (
    <section id="shortcuts" className="sec">
      <div className="sec-h">Keyboard shortcuts</div>
      <div className="kb-keybind-list">
        {allActions().map((action) => {
          const current = effective[action] ?? null
          const fallback = DEFAULT_KEYMAP[action] ?? null
          const conflictGroup = current ? accelToActions.get(current) ?? [] : []
          const conflicts = conflictGroup.filter((a) => a !== action)
          const isConflicting = conflicts.length > 0
          return (
            <div className="kb-keybind-row" key={action}>
              <div className="kb-keybind-row-main">
                <span className="kb-keybind-row-label">{actionLabel(action)}</span>
                {isConflicting && (
                  <span className="kb-keybind-row-conflict">
                    Also bound to{' '}
                    {conflicts.map(actionLabel).join(', ')}
                  </span>
                )}
              </div>
              <KeybindingInput
                value={current}
                defaultValue={fallback}
                conflicting={isConflicting}
                ariaLabel={actionLabel(action)}
                onChange={(next) => {
                  // `null` from the input means "reset to default".
                  onChange({ [action]: next ?? '' })
                }}
              />
            </div>
          )
        })}
      </div>
    </section>
  )
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / (1024 * 1024)).toFixed(1)} MB`
}
