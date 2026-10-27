/**
 * "Clear browsing data" dialog hosted by the `overlay-cleardata` webview so it
 * floats above the page webviews. The chrome sizes this webview to fill the window;
 * the dimmed backdrop captures click-outside and the dialog is a centered floating card.
 *
 * It wipes history and/or the chosen kinds of site data (cookies, cache,
 * storage) for the current workspace or all of them, reaching back over the
 * chosen time range, then emits `history-changed` so an open history page
 * refreshes, and closes itself.
 */

import { useCallback, useEffect, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { SiteDataKind } from '@/bindings/types'
import {
  ALL_SITE_DATA_KINDS,
  CLEAR_RANGES,
  SITE_DATA_KIND_ROWS,
  clearSince,
  type ClearRange,
} from '@/features/privacy/lib/clearData'
import { cx } from '@/lib/cx'
import { errorMessage } from '@/lib/errors'
import { currentPlatform } from '@/lib/platform'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'
import { focusSurface } from '@/hooks/useSurfaceFocus'
import { menusGlass } from '@/lib/glass'

type Scope = 'workspace' | 'all'

const FOCUS_EVENT = 'wis-overlay-cleardata-focus'
const CLOSED_EVENT = 'wis-overlay-cleardata-closed'
const HISTORY_CHANGED_EVENT = 'history-changed'

const SITE_DATA_UNAVAILABLE = currentPlatform() === 'linux'
const DEFAULT_KINDS: ReadonlyArray<SiteDataKind> = SITE_DATA_UNAVAILABLE ? [] : ALL_SITE_DATA_KINDS

export function ClearDataSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)

  const [scope, setScope] = useState<Scope>('workspace')
  const [range, setRange] = useState<ClearRange>('all')
  const [withHistory, setWithHistory] = useState(true)
  const [siteKinds, setSiteKinds] = useState<ReadonlyArray<SiteDataKind>>(DEFAULT_KINDS)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const activeWorkspaceId = state?.active_workspace_id ?? null
  const glassMenus = menusGlass(state?.settings)

  const close = useCallback(async () => {
    await commands.hideOverlay('cleardata').catch(() => undefined)
    await emit(CLOSED_EVENT).catch(() => undefined)
  }, [])

  // Reset to defaults each time the overlay is (re)focused/opened.
  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<void>(FOCUS_EVENT, () => {
      setScope('workspace')
      setRange('all')
      setWithHistory(true)
      setSiteKinds(DEFAULT_KINDS)
      setBusy(false)
      setError(null)
      focusSurface()
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        void close()
      }
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [close])

  const runClear = useCallback(async () => {
    setBusy(true)
    setError(null)
    const failures = new Set<string>()
    const failed = (e: unknown) => {
      failures.add(errorMessage(e))
    }

    const siteTargets =
      scope === 'workspace'
        ? activeWorkspaceId
          ? [activeWorkspaceId]
          : []
        : (state?.workspaces ?? []).filter((w) => !w.ephemeral).map((w) => w.id)

    const since = clearSince(range)
    if (withHistory) {
      await commands
        .clearHistory(scope === 'workspace' ? activeWorkspaceId ?? undefined : undefined, since)
        .catch(failed)
    }
    if (siteKinds.length > 0) {
      for (const id of siteTargets) {
        await commands.clearSiteData(id, siteKinds, since).catch(failed)
      }
    }
    await emit(HISTORY_CHANGED_EVENT).catch(() => undefined)

    if (failures.size > 0) {
      setError([...failures].join(' '))
      setBusy(false)
      return
    }
    await close()
  }, [scope, range, activeWorkspaceId, state?.workspaces, withHistory, siteKinds, close])

  const toggleKind = (kind: SiteDataKind) =>
    setSiteKinds((kinds) =>
      kinds.includes(kind) ? kinds.filter((k) => k !== kind) : [...kinds, kind],
    )

  const nothingSelected = !withHistory && siteKinds.length === 0

  return (
    <div
      className="kb-modal-backdrop"
      role="presentation"
      onPointerDown={(e) => {
        if (e.target === e.currentTarget) void close()
      }}
    >
      <div
        className={cx('kb-modal', 'kb-cbd', { 'glass-menus': glassMenus })}
        role="dialog"
        aria-modal="true"
        aria-label="Clear browsing data"
      >
        <h2 className="kb-modal-title">Clear browsing data</h2>

        <div className="kb-cbd-scope" role="tablist" aria-label="Scope">
          <button
            type="button"
            role="tab"
            aria-selected={scope === 'workspace'}
            className={cx({ 'is-on': scope === 'workspace' })}
            onClick={() => setScope('workspace')}
          >
            This workspace
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={scope === 'all'}
            className={cx({ 'is-on': scope === 'all' })}
            onClick={() => setScope('all')}
          >
            All workspaces
          </button>
        </div>

        <div className="kb-cbd-scope kb-cbd-range" role="tablist" aria-label="Time range">
          {CLEAR_RANGES.map((r) => (
            <button
              key={r.value}
              type="button"
              role="tab"
              aria-selected={range === r.value}
              className={cx({ 'is-on': range === r.value })}
              onClick={() => setRange(r.value)}
            >
              {r.label}
            </button>
          ))}
        </div>

        <CheckRow
          label="Browsing history"
          checked={withHistory}
          onToggle={() => setWithHistory((v) => !v)}
        />
        {SITE_DATA_KIND_ROWS.map((row) => (
          <CheckRow
            key={row.kind}
            label={row.label}
            hint={SITE_DATA_UNAVAILABLE ? 'Not available on Linux yet' : row.hint}
            checked={siteKinds.includes(row.kind)}
            disabled={SITE_DATA_UNAVAILABLE}
            onToggle={() => toggleKind(row.kind)}
          />
        ))}

        {error !== null && (
          <p className="kb-cbd-error" role="alert">
            Some data was not cleared: {error}
          </p>
        )}

        <div className="kb-modal-actions">
          <button type="button" className="kb-modal-btn" onClick={() => void close()}>
            Cancel
          </button>
          <button
            type="button"
            className="kb-modal-btn is-primary is-danger"
            disabled={nothingSelected || busy}
            onClick={() => void runClear()}
          >
            Clear
          </button>
        </div>
      </div>
    </div>
  )
}

interface CheckRowProps {
  label: string
  hint?: string
  checked: boolean
  disabled?: boolean
  onToggle: () => void
}

function CheckRow({ label, hint, checked, disabled = false, onToggle }: CheckRowProps) {
  return (
    <button
      type="button"
      className="kb-cbd-row"
      onClick={onToggle}
      aria-pressed={checked}
      disabled={disabled}
    >
      <span className={cx('kb-cbd-check', { 'is-on': checked })} aria-hidden>
        {checked ? '✓' : ''}
      </span>
      <span className="kb-cbd-text">
        <span className="kb-cbd-label">{label}</span>
        {hint ? <span className="kb-cbd-hint">{hint}</span> : null}
      </span>
    </button>
  )
}
