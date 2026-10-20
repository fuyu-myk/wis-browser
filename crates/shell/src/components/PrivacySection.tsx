import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type {
  PrivacyMode,
  UserSettings,
  UserSettingsPatch,
  WeekStart,
  Workspace,
  WorkspaceId,
} from '@/bindings/types'
import { Segmented, Switch } from '@/components/controls'
import { currentPlatform, isMacOS } from '@/lib/platform'
import { firstWeekday } from '@/lib/usage'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'
import { AutoplayRows } from '@/components/AutoplayRows'
import { FilterListsSection } from '@/components/FilterListsSection'
import { UserscriptsSection } from '@/components/UserscriptsSection'
import { AppLaunchSection } from '@/components/AppLaunchSection'
import { SitePermissionsSection } from '@/components/SitePermissionsSection'
import { SiteProtectionSection } from '@/components/SiteProtectionSection'
import { OPEN_CLEAR_DATA_EVENT } from '@/hooks/useClearDataOverlay'

const USAGE_IDLE_OPTIONS = [
  { value: '60' as const, label: '1 min' },
  { value: '120' as const, label: '2 min' },
  { value: '300' as const, label: '5 min' },
  { value: '600' as const, label: '10 min' },
]
type UsageIdle = (typeof USAGE_IDLE_OPTIONS)[number]['value']

const WEEK_START_OPTIONS = [
  { value: 'auto' as const, label: 'Automatic' },
  { value: 'sunday' as const, label: 'Sunday' },
  { value: 'monday' as const, label: 'Monday' },
] satisfies ReadonlyArray<{ value: WeekStart; label: string }>
const WEEKDAY_NAMES = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday']

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

interface PrivacySectionProps {
  settings: UserSettings
  activeWorkspace: Workspace | undefined
  onUpdate: (patch: UserSettingsPatch) => void
}

export function PrivacySection({ settings, activeWorkspace, onUpdate }: PrivacySectionProps) {
  const qc = useQueryClient()
  const [refreshStatus, setRefreshStatus] = useState<string | null>(null)
  const setWorkspaceMode = useMutation({
    mutationFn: ({ id, mode }: { id: WorkspaceId; mode: PrivacyMode }) =>
      commands.setWorkspacePrivacyMode(id, mode),
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })
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
  return (
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
            onChange={(default_privacy_mode) => onUpdate({ default_privacy_mode })}
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
            onChange={(always_private) => onUpdate({ always_private })}
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
            onChange={(persist_sessions) => onUpdate({ persist_sessions })}
            label="Stay logged in across restarts"
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Count time on sites</span>
            <span>
              Keep an on-device tally of active time per site for the Usage panel.
              Private workspaces are never counted; clearing history clears it.
            </span>
          </div>
          <Switch
            checked={settings.usage_tracking}
            onChange={(usage_tracking) => onUpdate({ usage_tracking })}
            label="Count time on sites"
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Stop counting after</span>
            <span>
              {currentPlatform() === 'linux'
                ? 'Not available on Linux yet: time counts while Wis is in front.'
                : 'Without keyboard or mouse input. The quiet stretch is not counted; playing audio keeps counting.'}
            </span>
          </div>
          <Segmented
            value={String(settings.usage_idle_timeout_secs) as UsageIdle}
            onChange={(v) => onUpdate({ usage_idle_timeout_secs: Number(v) })}
            options={USAGE_IDLE_OPTIONS}
            ariaLabel="Stop counting after"
            disabled={!settings.usage_tracking || currentPlatform() === 'linux'}
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Week starts on</span>
            <span>
              {`For the Usage panel and page. Automatic follows your region (${WEEKDAY_NAMES[firstWeekday('auto')]}).`}
            </span>
          </div>
          <Segmented
            value={settings.week_start}
            onChange={(week_start) => onUpdate({ week_start })}
            options={WEEK_START_OPTIONS}
            ariaLabel="Week starts on"
            disabled={!settings.usage_tracking}
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Fraudulent website warning</span>
            <span>
              {isMacOS()
                ? "Warn before suspected phishing or malware sites (WebKit's Apple-proxied Safe Browsing check; performs network lookups)."
                : "WebKit's Safe Browsing interstitial is macOS-only; the navigation guard still assesses risky links on this platform."}
            </span>
          </div>
          {isMacOS() && (
            <Switch
              checked={settings.fraud_warning_enabled}
              onChange={(fraud_warning_enabled) => onUpdate({ fraud_warning_enabled })}
              label="Fraudulent website warning"
            />
          )}
        </div>
        <div className="row">
          <div className="l">
            <span>Global Privacy Control</span>
            <span>
              Ask sites not to sell or share your data. Every page sees this signal,
              and it is sent only on unencrypted requests.
            </span>
          </div>
          <Switch
            checked={settings.global_privacy_control}
            onChange={(global_privacy_control) => onUpdate({ global_privacy_control })}
            label="Global Privacy Control"
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Streaming media support</span>
            <span>
              {isMacOS()
                ? 'Enable Media Source + Encrypted Media Extensions (adaptive video and the FairPlay DRM key system). On by default; turning it off may break video on some streaming sites.'
                : 'Media Source is always on here; the FairPlay toggle is macOS-only.'}
            </span>
          </div>
          {isMacOS() && (
            <Switch
              checked={settings.media_extensions_enabled}
              onChange={(media_extensions_enabled) => onUpdate({ media_extensions_enabled })}
              label="Streaming media support"
            />
          )}
        </div>
        <AutoplayRows settings={settings} onUpdate={onUpdate} />
        <div className="row">
          <div className="l">
            <span>Auto-update filter lists</span>
            <span>Fetch filter lists on launch, cache for 24 hours.</span>
          </div>
          <Switch
            checked={settings.filter_list_auto_update}
            onChange={(filter_list_auto_update) =>
              onUpdate({ filter_list_auto_update })
            }
            label="Auto-update filter lists"
          />
        </div>
        <FilterListsSection
          subscriptions={settings.filter_subscriptions}
          onChange={(filter_subscriptions) => onUpdate({ filter_subscriptions })}
          cosmeticRules={settings.user_cosmetic_rules}
          onCosmeticChange={(user_cosmetic_rules) => onUpdate({ user_cosmetic_rules })}
        />
        <UserscriptsSection
          scripts={settings.userscripts}
          onChange={(userscripts) => onUpdate({ userscripts })}
        />
        <SiteProtectionSection
          overrides={settings.site_protection_overrides}
          onChange={(site_protection_overrides) => onUpdate({ site_protection_overrides })}
        />
        <SitePermissionsSection
          permissions={settings.media_permissions}
          onChange={(media_permissions) => onUpdate({ media_permissions })}
        />
        <AppLaunchSection
          permissions={settings.app_launch_permissions}
          onChange={(app_launch_permissions) => onUpdate({ app_launch_permissions })}
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
  )
}
