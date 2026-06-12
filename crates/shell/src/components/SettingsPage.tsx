import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type {
  AccentColor,
  LayoutMode,
  PrivacyMode,
  RailSide,
  SearchEngine,
  ThemeMode,
  UaProfile,
  UserSettingsPatch,
  Widget,
  Workspace,
  WorkspaceId,
} from '@/bindings/types'
import { cx } from '@/lib/cx'
import { BROWSER_STATE_KEY, useBrowserState, useUpdateSettings } from '@/hooks/useBrowserState'
import {
  DownloadIcon,
  GlobeIcon,
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

import { Segmented, Switch } from '@/components/controls'
import { WidgetFavicon } from '@/components/WidgetFavicon'
import { WIDGET_CATALOG, makeWidget, type WidgetCatalogEntry } from '@/lib/widgets'
import { SiteDataSection } from '@/components/SiteDataSection'
import { PasswordsSection } from '@/components/PasswordsSection'
import { OPEN_CLEAR_DATA_EVENT } from '@/hooks/useClearDataOverlay'
import { BackgroundEditor } from '@/components/BackgroundEditor'
import { ThemesSection } from '@/components/ThemesSection'
import { KeybindingInput } from '@/components/KeybindingInput'
import { TaskManagerSection } from '@/components/TaskManagerSection'
import { FilterListsSection } from '@/components/FilterListsSection'

interface NavSection {
  id: string
  label: string
  icon: React.ReactNode
}

const NAV_SECTIONS: ReadonlyArray<NavSection> = [
  { id: 'appearance', label: 'Appearance', icon: <SunIcon /> },
  { id: 'layout', label: 'Layout', icon: <PanelTopIcon /> },
  { id: 'widgets', label: 'Sidebar widgets', icon: <GridIcon /> },
  { id: 'performance', label: 'Performance', icon: <MonitorIcon /> },
  { id: 'downloads', label: 'Downloads', icon: <DownloadIcon /> },
  { id: 'privacy', label: 'Privacy', icon: <LockIcon /> },
  { id: 'network', label: 'Network', icon: <LockIcon /> },
  { id: 'compatibility', label: 'Compatibility', icon: <GlobeIcon /> },
  { id: 'sitedata', label: 'Site data', icon: <LockIcon /> },
  { id: 'passwords', label: 'Passwords', icon: <LockIcon /> },
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

function GridIcon() {
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
      <rect x="3" y="3" width="7" height="7" rx="1.5" />
      <rect x="14" y="3" width="7" height="7" rx="1.5" />
      <rect x="3" y="14" width="7" height="7" rx="1.5" />
      <rect x="14" y="14" width="7" height="7" rx="1.5" />
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

const SEARCH_ENGINE_OPTIONS = [
  { value: 'duckduckgo' as const, label: 'DuckDuckGo' },
  { value: 'google' as const, label: 'Google' },
  { value: 'bing' as const, label: 'Bing' },
] satisfies ReadonlyArray<{ value: SearchEngine; label: string }>

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
    onSuccess: (summary) => {
      setRefreshStatus(summary)
    },
    onError: (err: unknown) => {
      const msg = err instanceof Error ? err.message : String(err)
      setRefreshStatus(`Refresh failed: ${msg}`)
    },
  })

  const mainRef = useRef<HTMLDivElement | null>(null)
  const [activeSection, setActiveSection] = useState<string>(NAV_SECTIONS[0].id)
  // Non-null while a click-initiated smooth scroll is in flight.
  const scrollLockRef = useRef<number | null>(null)
  const navRef = useRef<HTMLElement | null>(null)
  const [navInd, setNavInd] = useState<{
    top: number
    height: number
    left: number
    width: number
  } | null>(null)
  useLayoutEffect(() => {
    const btn = navRef.current?.querySelector<HTMLElement>('button.is-active')
    if (!btn) {
      setNavInd(null)
      return
    }
    setNavInd({
      top: btn.offsetTop,
      height: btn.offsetHeight,
      left: btn.offsetLeft,
      width: btn.offsetWidth,
    })
  }, [activeSection])


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
        if (scrollLockRef.current !== null) return
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
  const navOnRight =
    settings.layout === 'sidebar_right' ||
    (settings.layout === 'top_tabs' && settings.top_tabs_rail_side === 'right')

  const scrollTo = (id: string) => {
    // Optimistic: the indicator moves on the click.
    setActiveSection(id)
    if (scrollLockRef.current !== null) window.clearTimeout(scrollLockRef.current)
    scrollLockRef.current = window.setTimeout(() => {
      scrollLockRef.current = null
    }, 700)
    const el = mainRef.current?.querySelector(`#${id}`)
    if (el) el.scrollIntoView({ behavior: 'smooth', block: 'start' })
  }

  return (
    <div
      className={cx('kb-settings-page', { 'nav-right': navOnRight })}
      aria-label="Settings"
      onFocus={(e) => {
        const t = e.target
        if (t instanceof HTMLInputElement && (t.type === 'number' || t.type === 'text')) {
          t.select()
          t.dataset.selectAll = '1'
        }
      }}
      onMouseUp={(e) => {
        const t = e.target
        if (t instanceof HTMLInputElement && t.dataset.selectAll === '1') {
          delete t.dataset.selectAll
          e.preventDefault()
        }
      }}
    >
      <aside className="kb-settings-nav" aria-label="Settings sections" ref={navRef}>
        <div className="kb-settings-nav-title">Settings</div>
        {navInd !== null && (
          <span
            className="kb-settings-nav-ind"
            style={{
              top: navInd.top,
              height: navInd.height,
              left: navInd.left,
              width: navInd.width,
            }}
            aria-hidden
          />
        )}
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
            <span>Motion</span>
            <span>
              Reduced keeps the same timings without the springiness; Off
              disables non-essential animation. Follows the OS reduce-motion
              preference when set to Full.
            </span>
          </div>
          <Segmented
            value={settings.motion}
            onChange={(motion) => update.mutate({ motion })}
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
        <ThemesSection />
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
            <span>Collapsible sidebar</span>
            <span>Expand on edge hover or click</span>
          </div>
          <Switch
            checked={settings.sidebar_collapsible}
            onChange={(sidebar_collapsible) => update.mutate({ sidebar_collapsible })}
            label="Collapsible sidebar"
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Search engine</span>
            <span>Used for address-bar searches and suggestions</span>
          </div>
          <Segmented
            value={settings.search_engine}
            onChange={(search_engine) => update.mutate({ search_engine })}
            options={SEARCH_ENGINE_OPTIONS}
            ariaLabel="Default search engine"
          />
        </div>
      </section>

      <SidebarWidgetsSection
        widgets={settings.sidebar_widgets}
        onUpdate={(patch) => update.mutate(patch)}
      />

      <section id="performance" className="sec">
        <div className="sec-h">Performance</div>
        <div className="row">
          <div className="l">
            <span>Sleep inactive tabs</span>
            <span>
              Discard background tabs after a while to free memory. They reload
              when you switch back. The active tab and pinned tabs never sleep.
            </span>
          </div>
          <Switch
            checked={settings.tab_sleep_enabled}
            onChange={(tab_sleep_enabled) => update.mutate({ tab_sleep_enabled })}
            label="Sleep inactive tabs"
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Sleep after</span>
            <span>Idle minutes a background tab waits before sleeping.</span>
          </div>
          <input
            type="number"
            className="kb-num-input"
            min={1}
            max={1440}
            step={1}
            value={settings.tab_sleep_minutes}
            disabled={!settings.tab_sleep_enabled}
            onChange={(e) => {
              const n = Math.round(Number(e.currentTarget.value))
              if (!Number.isFinite(n)) return
              update.mutate({ tab_sleep_minutes: Math.min(1440, Math.max(1, n)) })
            }}
            aria-label="Sleep after minutes"
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Keep widget panels loaded</span>
            <span>
              Dismissed widget panels stay loaded for instant reopening. Turn
              off to free each panel's memory the moment it closes; logins
              persist either way.
            </span>
          </div>
          <Switch
            checked={settings.widget_keep_loaded}
            onChange={(widget_keep_loaded) => update.mutate({ widget_keep_loaded })}
            label="Keep widget panels loaded"
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Reclaim hidden panels after</span>
            <span>Minutes a hidden widget panel stays loaded before it is closed.</span>
          </div>
          <input
            type="number"
            className="kb-num-input"
            min={1}
            max={1440}
            step={1}
            value={settings.widget_sleep_minutes}
            disabled={!settings.widget_keep_loaded}
            onChange={(e) => {
              const n = Math.round(Number(e.currentTarget.value))
              if (!Number.isFinite(n)) return
              update.mutate({ widget_sleep_minutes: Math.min(1440, Math.max(1, n)) })
            }}
            aria-label="Widget panel reclaim minutes"
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Cap hidden panel memory</span>
            <span>
              Reclaim hidden widget panels oldest-first when together they
              hold more memory than the cap.
            </span>
          </div>
          <Switch
            checked={settings.widget_memory_cap_enabled}
            onChange={(widget_memory_cap_enabled) =>
              update.mutate({ widget_memory_cap_enabled })
            }
            label="Cap hidden panel memory"
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Memory cap</span>
            <span>Combined footprint (MB) hidden panels may hold.</span>
          </div>
          <MemoryCapInput
            value={settings.widget_memory_cap_mb}
            disabled={!settings.widget_keep_loaded || !settings.widget_memory_cap_enabled}
            onCommit={(widget_memory_cap_mb) => update.mutate({ widget_memory_cap_mb })}
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Limit loaded widget panels</span>
            <span>
              Cap how many recently used widget panels stay loaded for instant
              reopening; the oldest is reclaimed first. Turn off for no limit.
            </span>
          </div>
          <Switch
            checked={settings.widget_panel_limit_enabled}
            onChange={(widget_panel_limit_enabled) =>
              update.mutate({ widget_panel_limit_enabled })
            }
            label="Limit loaded widget panels"
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Keep at most</span>
            <span>Loaded widget panels kept before the oldest is reclaimed.</span>
          </div>
          <input
            type="number"
            className="kb-num-input"
            min={1}
            max={50}
            step={1}
            value={settings.widget_panel_limit}
            disabled={!settings.widget_panel_limit_enabled}
            onChange={(e) => {
              const n = Math.round(Number(e.currentTarget.value))
              if (!Number.isFinite(n)) return
              update.mutate({ widget_panel_limit: Math.min(50, Math.max(1, n)) })
            }}
            aria-label="Loaded widget panel limit"
          />
        </div>
        <LiveWidgetPanelsRow
          widgets={settings.sidebar_widgets}
          limitEnabled={settings.widget_panel_limit_enabled}
          limit={settings.widget_panel_limit}
        />
        <TaskManagerSection />
      </section>

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
            onChange={(download_ask_where) => update.mutate({ download_ask_where })}
            label="Ask where to save each download"
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
            <span>Always launch private</span>
            <span>Open into a private (ephemeral) workspace on startup. Nothing is saved to disk.</span>
          </div>
          <Switch
            checked={settings.always_private}
            onChange={(always_private) => update.mutate({ always_private })}
            label="Always launch private"
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Stay logged in across restarts</span>
            <span>
              Save login sessions (encrypted) so sites stay signed in
              after a restart. Restored sessions auto-expire after two weeks.
              Turning this off ends sessions on quit and wipes any saved ones.
            </span>
          </div>
          <Switch
            checked={settings.persist_sessions}
            onChange={(persist_sessions) => update.mutate({ persist_sessions })}
            label="Stay logged in across restarts"
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Fraudulent website warning</span>
            <span>
              Warn before suspected phishing or malware sites (WebKit's
              Apple-proxied Safe Browsing check; performs network lookups).
            </span>
          </div>
          <Switch
            checked={settings.fraud_warning_enabled}
            onChange={(fraud_warning_enabled) => update.mutate({ fraud_warning_enabled })}
            label="Fraudulent website warning"
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Auto-update filter lists</span>
            <span>Fetch filter lists on launch, cache for 24 hours.</span>
          </div>
          <Switch
            checked={settings.filter_list_auto_update}
            onChange={(filter_list_auto_update) =>
              update.mutate({ filter_list_auto_update })
            }
            label="Auto-update filter lists"
          />
        </div>
        <FilterListsSection
          subscriptions={settings.filter_subscriptions}
          onChange={(filter_subscriptions) => update.mutate({ filter_subscriptions })}
        />
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
            {refreshLists.isPending ? 'Refreshing...' : 'Refresh'}
          </button>
        </div>
        <div className="row">
          <div className="l">
            <span>Clear browsing data</span>
            <span>Remove history, cookies, cache and local storage.</span>
          </div>
          <button
            type="button"
            className="kb-btn"
            onClick={() => window.dispatchEvent(new CustomEvent(OPEN_CLEAR_DATA_EVENT))}
          >
            Clear…
          </button>
        </div>
      </section>

      <NetworkSection workspace={activeWorkspace} />

      <CompatibilitySection
        profile={settings.user_agent_profile}
        overrides={settings.ua_site_overrides}
        onUpdate={(patch) => update.mutate(patch)}
      />

      <SiteDataSection
        workspaceId={activeWorkspace?.id ?? null}
        workspaceName={activeWorkspace?.name ?? 'workspace'}
      />

      <PasswordsSection />

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

const DEFAULT_WIDGET_MEMORY_CAP_MB = 500

/** Free-typing draft input for the memory cap: clamps to 100..8192 only on
 * blur/Enter, and an emptied field resets to the default. The page-level
 * focus handler already selects the text on click. */
function MemoryCapInput({
  value,
  disabled,
  onCommit,
}: {
  value: number
  disabled: boolean
  onCommit: (mb: number) => void
}) {
  const [draft, setDraft] = useState<string | null>(null)
  const commit = () => {
    if (draft === null) return
    const trimmed = draft.trim()
    setDraft(null)
    const n = Math.round(Number(trimmed))
    if (trimmed === '' || !Number.isFinite(n)) {
      onCommit(DEFAULT_WIDGET_MEMORY_CAP_MB)
      return
    }
    onCommit(Math.min(8192, Math.max(100, n)))
  }
  return (
    <input
      type="number"
      className="kb-num-input"
      min={100}
      max={8192}
      step={50}
      value={draft ?? String(value)}
      disabled={disabled}
      onChange={(e) => setDraft(e.currentTarget.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === 'Enter') e.currentTarget.blur()
      }}
      aria-label="Hidden panel memory cap in MB"
    />
  )
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / (1024 * 1024)).toFixed(1)} MB`
}

// ─── Compatibility (browser identity / user agent) ─────────────────────────

type SimpleUa = 'safari' | 'chrome' | 'default'

const UA_OPTIONS = [
  { value: 'safari' as const, label: 'Safari' },
  { value: 'chrome' as const, label: 'Chrome' },
  { value: 'default' as const, label: 'Engine' },
] satisfies ReadonlyArray<{ value: SimpleUa; label: string }>

function uaProfileLabel(p: UaProfile): string {
  if (p === 'safari') return 'Safari'
  if (p === 'chrome') return 'Chrome'
  if (p === 'default') return 'Engine default'
  return 'Custom'
}

/** Reduce a pasted URL or `host/path` to a `www.`-stripped, lowercased host. */
function normalizeHostInput(raw: string): string {
  let s = raw.trim().toLowerCase()
  if (s === '') return ''
  if (s.includes('://')) {
    try {
      s = new URL(s).hostname
    } catch {
      // Not a parseable URL; fall through and treat the text as a host.
    }
  }
  s = s.split('/')[0]
  return s.replace(/^www\./, '')
}

interface CompatibilitySectionProps {
  profile: UaProfile
  overrides: Record<string, UaProfile>
  onUpdate: (patch: UserSettingsPatch) => void
}

function CompatibilitySection({ profile, overrides, onUpdate }: CompatibilitySectionProps) {
  const [host, setHost] = useState('')
  const [newProfile, setNewProfile] = useState<SimpleUa>('chrome')

  const globalValue: SimpleUa = typeof profile === 'string' ? profile : 'safari'
  const entries = Object.entries(overrides).sort(([a], [b]) => a.localeCompare(b))

  const addOverride = () => {
    const h = normalizeHostInput(host)
    if (!h) return
    onUpdate({ ua_site_overrides: { ...overrides, [h]: newProfile } })
    setHost('')
  }

  const removeOverride = (h: string) => {
    const next = { ...overrides }
    delete next[h]
    onUpdate({ ua_site_overrides: next })
  }

  return (
    <section id="compatibility" className="sec">
      <div className="sec-h">Compatibility</div>
      <div className="row">
        <div className="l">
          <span>Browser identity</span>
          <span>
            How Wis identifies to sites. Safari is recommended (the engine is
            WebKit). Switch a stubborn site to Chrome below if it still reports
            an unsupported browser.
          </span>
        </div>
        <Segmented
          value={globalValue}
          onChange={(v) => onUpdate({ user_agent_profile: v })}
          options={UA_OPTIONS}
          ariaLabel="Default browser identity"
        />
      </div>
      <div className="row">
        <div className="l">
          <span>Per-site identity</span>
          <span>Override the identity for specific sites that need it.</span>
        </div>
      </div>
      <div className="kb-ua-add">
        <input
          type="text"
          className="kb-text-input"
          placeholder="example.com"
          value={host}
          onChange={(e) => setHost(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') addOverride()
          }}
          aria-label="Site host"
        />
        <Segmented
          value={newProfile}
          onChange={(v) => setNewProfile(v)}
          options={UA_OPTIONS}
          ariaLabel="Override identity"
        />
        <button
          type="button"
          className="kb-btn"
          onClick={addOverride}
          disabled={normalizeHostInput(host) === ''}
        >
          Add
        </button>
      </div>
      {entries.length > 0 && (
        <ul className="kb-ua-list">
          {entries.map(([h, p]) => (
            <li key={h} className="kb-ua-item">
              <span className="kb-ua-host">{h}</span>
              <span className="kb-ua-prof">{uaProfileLabel(p)}</span>
              <button
                type="button"
                className="kb-ua-remove"
                onClick={() => removeOverride(h)}
                aria-label={`Remove override for ${h}`}
              >
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}

// ─── Sidebar widgets ───────────────────────────────────────────────────────

interface SidebarWidgetsSectionProps {
  widgets: Widget[]
  onUpdate: (patch: UserSettingsPatch) => void
}

function SidebarWidgetsSection({ widgets, onUpdate }: SidebarWidgetsSectionProps) {
  const enabledKinds = new Set(widgets.map((w) => w.kind))

  const toggle = (entry: WidgetCatalogEntry) => {
    const next = enabledKinds.has(entry.kind)
      ? widgets.filter((w) => w.kind !== entry.kind)
      : [...widgets, makeWidget(entry)]
    onUpdate({ sidebar_widgets: next })
  }

  const chip = (entry: WidgetCatalogEntry) => {
    const on = enabledKinds.has(entry.kind)
    return (
      <button
        key={entry.kind}
        type="button"
        role="switch"
        aria-checked={on}
        className={cx('kb-widget-chip', { on })}
        onClick={() => toggle(entry)}
      >
        <WidgetFavicon url={entry.url} label={entry.label} />
        <span>{entry.label}</span>
      </button>
    )
  }

  const social = WIDGET_CATALOG.filter((e) => e.category === 'social')
  const ai = WIDGET_CATALOG.filter((e) => e.category === 'ai')

  return (
    <section id="widgets" className="sec">
      <div className="sec-h">Sidebar widgets</div>
      <div className="row">
        <div className="l">
          <span>Quick launch</span>
          <span>
            Choose the social apps and AI providers to show in the sidebar.
            Clicking one opens it in a split pane on the sidebar side.
          </span>
        </div>
      </div>
      <div className="kb-widget-group-label">Social</div>
      <div className="kb-widget-chips">{social.map(chip)}</div>
      <div className="kb-widget-sep" aria-hidden />
      <div className="kb-widget-group-label">AI providers</div>
      <div className="kb-widget-chips">{ai.map(chip)}</div>
    </section>
  )
}

// ─── Live widget panels (Performance) ──────────────────────────────────────

const WIDGET_PANELS_KEY = ['widget-panels']
const WIDGET_PANELS_CHANGED_EVENT = 'widget-panels-changed'

interface LiveWidgetPanelsRowProps {
  widgets: Widget[]
  limitEnabled: boolean
  limit: number
}

/**
 * Performance row showing how many widget panels are currently kept loaded
 * (visible or hidden for instant reopening), with a one-click reclaim.
 * Counts come from Rust (the actual live webviews), refreshed on the
 * `widget-panels-changed` event so LRU evictions and idle destroys reflect.
 */
function LiveWidgetPanelsRow({ widgets, limitEnabled, limit }: LiveWidgetPanelsRowProps) {
  const qc = useQueryClient()
  const panels = useQuery({
    queryKey: WIDGET_PANELS_KEY,
    queryFn: () => commands.listWidgetPanels(),
  })

  useEffect(() => {
    let un: UnlistenFn | null = null
    let cancelled = false
    void listen<void>(WIDGET_PANELS_CHANGED_EVENT, () =>
      qc.invalidateQueries({ queryKey: WIDGET_PANELS_KEY }),
    ).then((fn) => (cancelled ? fn() : (un = fn)))
    return () => {
      cancelled = true
      un?.()
    }
  }, [qc])

  const ids = panels.data ?? []
  const names = ids.map((id) => widgets.find((w) => w.id === id)?.label ?? 'Widget')
  const capLabel = limitEnabled ? `of up to ${limit} ` : ''
  const description =
    ids.length === 0
      ? 'None right now. Recently used widgets stay loaded for instant reopening, then sleep.'
      : `${names.join(', ')} — ${ids.length} ${capLabel}kept loaded for instant reopening.`

  return (
    <div className="row">
      <div className="l">
        <span>Loaded widget panels</span>
        <span>{description}</span>
      </div>
      <button
        type="button"
        className="kb-btn"
        disabled={ids.length === 0}
        onClick={() => {
          void commands
            .closeAllWidgetPanels()
            .catch(() => undefined)
            .finally(() => qc.invalidateQueries({ queryKey: WIDGET_PANELS_KEY }))
        }}
      >
        Close all
      </button>
    </div>
  )
}

// ─── Network (built-in Tor VPN) ────────────────────────────────────────────

type ProbeStatus =
  | { kind: 'idle' }
  | { kind: 'connecting' }
  | { kind: 'ok' }
  | { kind: 'fail'; msg: string }

function NetworkSection({ workspace }: { workspace: Workspace | undefined }) {
  const qc = useQueryClient()
  const current = workspace?.upstream_proxy ?? null
  const on = current != null
  const upstreamKey = JSON.stringify(current ?? null)
  const [status, setStatus] = useState<ProbeStatus>({ kind: 'idle' })

  // Confirm the connection whenever the VPN is on.
  useEffect(() => {
    let cancelled = false
    if (!current) {
      setStatus({ kind: 'idle' })
      return
    }
    setStatus({ kind: 'connecting' })
    commands.testUpstream(current).then(
      () => {
        if (!cancelled) setStatus({ kind: 'ok' })
      },
      (e: unknown) => {
        if (!cancelled) {
          setStatus({ kind: 'fail', msg: e instanceof Error ? e.message : String(e) })
        }
      },
    )
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [upstreamKey])

  if (!workspace) return null
  const id = workspace.id

  const toggle = (next: boolean) => {
    if (next) setStatus({ kind: 'connecting' }) // optimistic
    void commands
      .setWorkspaceUpstream(id, next ? { kind: 'tor' } : null)
      .catch(() => undefined)
      .finally(() => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }))
  }

  return (
    <section id="network" className="sec">
      <div className="sec-h">Network</div>
      <div className="row">
        <div className="l">
          <span>VPN (Tor)</span>
          <span>Route this workspace through the built-in Tor network. First connect bootstraps and can take a few seconds.</span>
        </div>
        <Switch checked={on} onChange={toggle} label="VPN" />
      </div>
      {on && (
        <div className="row">
          <div className="l">
            <span>Connection</span>
            <span>Status of the Tor connection for this workspace.</span>
          </div>
          <ProbeBadge status={status} />
        </div>
      )}
    </section>
  )
}

function ProbeBadge({ status }: { status: ProbeStatus }) {
  if (status.kind === 'idle') return <span className="kb-net-status is-idle">Off</span>
  if (status.kind === 'connecting')
    return <span className="kb-net-status is-checking">Connecting…</span>
  if (status.kind === 'ok')
    return (
      <span className="kb-net-status is-ok">
        <span className="kb-net-dot" />
        Connected
      </span>
    )
  return (
    <span className="kb-net-status is-fail" title={status.msg}>
      <span className="kb-net-dot" />
      Can’t connect
    </span>
  )
}
