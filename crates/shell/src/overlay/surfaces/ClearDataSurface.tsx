/**
 * "Clear browsing data" dialog hosted by the `overlay-cleardata` webview so it
 * floats above the page webviews. The chrome sizes this webview to fill the window;
 * the dimmed backdrop captures click-outside and the dialog is a centered floating card.
 *
 * It wipes history and/or site data (cookies, cache, local storage) for the
 * current workspace or all of them, then emits `history-changed` so an open
 * history page refreshes, and closes itself.
 */

import { useCallback, useEffect, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import { cx } from '@/lib/cx'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'
import { menusGlass } from '@/lib/glass'

type Scope = 'workspace' | 'all'

const FOCUS_EVENT = 'wis-overlay-cleardata-focus'
const CLOSED_EVENT = 'wis-overlay-cleardata-closed'
const HISTORY_CHANGED_EVENT = 'history-changed'

export function ClearDataSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)

  const [scope, setScope] = useState<Scope>('workspace')
  const [withHistory, setWithHistory] = useState(true)
  const [withSiteData, setWithSiteData] = useState(true)
  const [busy, setBusy] = useState(false)

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
      setWithHistory(true)
      setWithSiteData(true)
      setBusy(false)
      void commands.focusSelf().catch(() => undefined)
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
    try {
      const siteTargets =
        scope === 'workspace'
          ? activeWorkspaceId
            ? [activeWorkspaceId]
            : []
          : (state?.workspaces ?? []).filter((w) => !w.ephemeral).map((w) => w.id)

      if (withHistory) {
        await commands
          .clearHistory(scope === 'workspace' ? activeWorkspaceId ?? undefined : undefined)
          .catch(() => undefined)
      }
      if (withSiteData) {
        for (const id of siteTargets) {
          await commands.clearSiteData(id).catch(() => undefined)
        }
      }
      await emit(HISTORY_CHANGED_EVENT).catch(() => undefined)
    } finally {
      await close()
    }
  }, [scope, activeWorkspaceId, state?.workspaces, withHistory, withSiteData, close])

  const nothingSelected = !withHistory && !withSiteData

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

        <CheckRow
          label="Browsing history"
          checked={withHistory}
          onToggle={() => setWithHistory((v) => !v)}
        />
        <CheckRow
          label="Cookies and site data"
          hint="Cookies, cache and local storage"
          checked={withSiteData}
          onToggle={() => setWithSiteData((v) => !v)}
        />

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
  onToggle: () => void
}

function CheckRow({ label, hint, checked, onToggle }: CheckRowProps) {
  return (
    <button type="button" className="kb-cbd-row" onClick={onToggle} aria-pressed={checked}>
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
