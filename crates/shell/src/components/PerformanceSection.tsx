import { useEffect, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { TabArchiveAfter, UserSettings, UserSettingsPatch, Widget } from '@/bindings/types'
import { Segmented, Switch } from '@/components/controls'
import { TaskManagerSection } from '@/components/TaskManagerSection'

const ARCHIVE_OPTIONS = [
  { value: 'off' as const, label: 'Off' },
  { value: 'twelve_hours' as const, label: '12 hours' },
  { value: 'one_day' as const, label: '1 day' },
  { value: 'seven_days' as const, label: '7 days' },
  { value: 'thirty_days' as const, label: '30 days' },
] satisfies ReadonlyArray<{ value: TabArchiveAfter; label: string }>

interface PerformanceSectionProps {
  settings: UserSettings
  onUpdate: (patch: UserSettingsPatch) => void
}

export function PerformanceSection({ settings, onUpdate }: PerformanceSectionProps) {
  return (
      <section id="performance" className="sec">
        <div className="sec-h">Performance</div>
        <div className="row">
          <div className="l">
            <span>Sleep inactive tabs</span>
            <span>
              Discard background tabs after a while to free memory. They reload
              when you switch back. The active tab never sleeps.
            </span>
          </div>
          <Switch
            checked={settings.tab_sleep_enabled}
            onChange={(tab_sleep_enabled) => onUpdate({ tab_sleep_enabled })}
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
              onUpdate({ tab_sleep_minutes: Math.min(1440, Math.max(1, n)) })
            }}
            aria-label="Sleep after minutes"
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Archive idle tabs</span>
            <span>
              Close tabs you haven't opened in this long into Recently closed.
              Pinned tabs, tabs in folders and private workspaces' tabs are unaffected.
            </span>
          </div>
          <Segmented
            value={settings.tab_archive_after}
            onChange={(tab_archive_after) => onUpdate({ tab_archive_after })}
            options={ARCHIVE_OPTIONS}
            ariaLabel="Archive idle tabs after"
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
            onChange={(widget_keep_loaded) => onUpdate({ widget_keep_loaded })}
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
              onUpdate({ widget_sleep_minutes: Math.min(1440, Math.max(1, n)) })
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
              onUpdate({ widget_memory_cap_enabled })
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
            onCommit={(widget_memory_cap_mb) => onUpdate({ widget_memory_cap_mb })}
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
              onUpdate({ widget_panel_limit_enabled })
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
              onUpdate({ widget_panel_limit: Math.min(50, Math.max(1, n)) })
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
    staleTime: 0,
    refetchOnMount: 'always',
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
