import { useEffect, useId, useMemo, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { HistoryEntry, Tab, WorkspaceId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { visibleClosedTabs } from '@/features/history/closedTabs'
import { formatTime, groupByDay, hostOf } from '@/features/history/dayGroups'
import { SearchIcon, XIcon } from '@/lib/icons'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useClosedTabs } from '@/features/history/useClosedTabs'
import { useNavigateOrCreateTab } from '@/hooks/useNavigateOrCreateTab'
import { useScrollMemory } from '@/hooks/useScrollMemory'
import { ClosedTabsList } from '@/features/history/ClosedTabsList'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { moveFocusByArrow } from '@/lib/arrowNav'

const HISTORY_KEY = ['history'] as const
const SEARCH_DEBOUNCE_MS = 250
const PAGE_LIMIT = 500

type Scope = 'workspace' | 'all'
type View = 'history' | 'closed'

interface HistoryPageProps {
  /** The tab currently showing this page (navigated in place on click). */
  activeTab: Tab | null
}

export function HistoryPage({ activeTab }: HistoryPageProps) {
  const { data: state } = useBrowserState()
  const qc = useQueryClient()
  const activeWorkspaceId = state?.active_workspace_id ?? null

  const [view, setView] = useState<View>('history')
  const [search, setSearch] = useState('')
  const debouncedSearch = useDebounce(search, SEARCH_DEBOUNCE_MS)
  const [scope, setScope] = useState<Scope>('workspace')

  const [selectMode, setSelectMode] = useState(false)
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const [confirmClear, setConfirmClear] = useState(false)

  const navigate = useNavigateOrCreateTab(activeWorkspaceId, activeTab)
  const scrollKey = activeTab?.id ?? 'history'
  const attachHistoryScroll = useScrollMemory(`${scrollKey}:history`)
  const attachClosedScroll = useScrollMemory(`${scrollKey}:closed`)

  const queryWorkspaceId: WorkspaceId | null =
    scope === 'workspace' ? activeWorkspaceId : null

  const { data: entries = [], isLoading } = useQuery({
    queryKey: [...HISTORY_KEY, scope, queryWorkspaceId, debouncedSearch],
    queryFn: () =>
      commands.getHistory({
        search: debouncedSearch,
        workspace_id: queryWorkspaceId,
        limit: PAGE_LIMIT,
      }),
    staleTime: 0,
    refetchOnMount: 'always',
    enabled: view === 'history',
  })
  const privateWorkspace =
    state?.workspaces.find((w) => w.id === activeWorkspaceId)?.ephemeral ?? false
  const closed = useClosedTabs(view === 'closed' && !privateWorkspace)
  const closedInScope = useMemo(
    () => visibleClosedTabs(closed.entries, queryWorkspaceId, ''),
    [closed.entries, queryWorkspaceId],
  )
  const closedEntries = useMemo(
    () => visibleClosedTabs(closedInScope, null, search),
    [closedInScope, search],
  )

  // Live updates while the page is open: the backend emits `history-changed`
  // after each recorded visit.
  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen('history-changed', () => {
      void qc.invalidateQueries({ queryKey: HISTORY_KEY })
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [qc])

  const workspaceName = useMemo(() => {
    const map = new Map<string, string>()
    state?.workspaces.forEach((w) => map.set(w.id, w.name))
    return map
  }, [state?.workspaces])

  const refresh = () => qc.invalidateQueries({ queryKey: HISTORY_KEY })

  const deleteEntries = useMutation({
    mutationFn: (ids: number[]) => commands.deleteHistory(ids),
    onSuccess: refresh,
  })
  const clearScope = useMutation({
    mutationFn: () => {
      const workspace = scope === 'workspace' ? activeWorkspaceId ?? undefined : undefined
      return view === 'history'
        ? commands.clearHistory(workspace)
        : commands.clearClosedTabs(workspace)
    },
    onSuccess: refresh,
  })

  // A scope change can hide selected rows; drop the selection to avoid
  // acting on entries no longer visible.
  useEffect(() => {
    setSelected(new Set())
    setSelectMode(false)
  }, [scope])

  const groups = useMemo(() => groupByDay(entries, (entry) => entry.visited_at), [entries])
  const closedGroups = useMemo(
    () => groupByDay(closedEntries, (entry) => entry.closed_at),
    [closedEntries],
  )

  const exitSelect = () => {
    setSelectMode(false)
    setSelected(new Set())
  }

  useEffect(() => {
    if (!selectMode) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        setSelectMode(false)
        setSelected(new Set())
      }
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [selectMode])
  const toggle = (id: number) => {
    setSelected((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }
  const allSelected = entries.length > 0 && selected.size === entries.length
  const toggleAll = () => {
    setSelected(allSelected ? new Set() : new Set(entries.map((e) => e.id)))
  }
  const deleteSelected = () => {
    if (selected.size === 0) return
    deleteEntries.mutate([...selected])
    exitSelect()
  }

  const ids = useId()
  const showView = (next: View) => {
    if (next === view) return
    exitSelect()
    setView(next)
  }

  const clearLabel = scope === 'workspace' ? 'this workspace' : 'all workspaces'
  const offeredClosed = privateWorkspace ? [] : closedEntries
  const shown = view === 'history' ? entries.length : privateWorkspace ? 0 : closedInScope.length

  return (
    <div className="kb-history-page">
      <div className="inner">
        <header className="kb-history-head">
          <h1 className="kb-sr-only">History</h1>
          <div className="kb-history-head-row">
            <div
              className="kb-history-views"
              role="tablist"
              aria-label="History view"
              onKeyDown={(e) => moveFocusByArrow(e, e.currentTarget, 'button', 'horizontal')}
            >
              <ViewTab ids={ids} view="history" current={view} onSelect={showView}>
                History
              </ViewTab>
              <ViewTab ids={ids} view="closed" current={view} onSelect={showView}>
                Recently closed
              </ViewTab>
            </div>
            {selectMode ? (
              <div className="kb-history-actions">
                <span className="kb-history-count">{selected.size} selected</span>
                <button
                  type="button"
                  className="kb-history-btn"
                  onClick={toggleAll}
                  disabled={entries.length === 0}
                >
                  {allSelected ? 'Deselect all' : 'Select all'}
                </button>
                <button
                  type="button"
                  className="kb-history-btn is-danger"
                  onClick={deleteSelected}
                  disabled={selected.size === 0}
                >
                  Delete
                </button>
                <button type="button" className="kb-history-btn" onClick={exitSelect}>
                  Cancel
                </button>
              </div>
            ) : (
              <div className="kb-history-actions">
                {view === 'history' ? (
                  <button
                    type="button"
                    className="kb-history-btn"
                    onClick={() => setSelectMode(true)}
                    disabled={entries.length === 0}
                  >
                    Select
                  </button>
                ) : null}
                <button
                  type="button"
                  className="kb-history-btn is-danger"
                  onClick={() => setConfirmClear(true)}
                  disabled={shown === 0 || clearScope.isPending}
                >
                  Clear all
                </button>
              </div>
            )}
          </div>
          <div className="kb-history-head-row">
            <div
              className="kb-history-scope"
              role="radiogroup"
              aria-label="History scope"
              onKeyDown={(e) => moveFocusByArrow(e, e.currentTarget, 'button', 'horizontal')}
            >
              <button
                type="button"
                role="radio"
                aria-checked={scope === 'workspace'}
                className={cx({ 'is-on': scope === 'workspace' })}
                onClick={() => setScope('workspace')}
              >
                This workspace
              </button>
              <button
                type="button"
                role="radio"
                aria-checked={scope === 'all'}
                className={cx({ 'is-on': scope === 'all' })}
                onClick={() => setScope('all')}
              >
                All workspaces
              </button>
            </div>
            <div className="kb-history-search">
              <SearchIcon />
              <input
                type="text"
                placeholder={view === 'history' ? 'Search history' : 'Search recently closed'}
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                autoFocus
                spellCheck={false}
              />
            </div>
          </div>
        </header>

        <div
          className="kb-history-panel"
          role="tabpanel"
          id={`${ids}-panel-${view}`}
          aria-labelledby={`${ids}-tab-${view}`}
        >
          {view === 'closed' ? (
            closed.isLoading ? null : offeredClosed.length === 0 ? (
              <p className="kb-history-empty">
                {privateWorkspace
                  ? "Private workspaces don't reopen closed tabs."
                  : search.trim()
                    ? 'No matching tabs.'
                    : 'No closed tabs yet.'}
              </p>
            ) : (
              <ClosedTabsList
                groups={closedGroups}
                workspaceNames={scope === 'all' ? workspaceName : undefined}
                onReopen={(id) => void commands.reopenClosedTab(id).catch(() => undefined)}
                onForget={(id) => void commands.forgetClosedTab(id).catch(() => undefined)}
                attachScroll={attachClosedScroll}
              />
            )
          ) : isLoading ? null : entries.length === 0 ? (
            <p className="kb-history-empty">
              {debouncedSearch ? 'No matching history.' : 'No history yet.'}
            </p>
          ) : (
            <div className="kb-history-groups" ref={attachHistoryScroll}>
              {groups.map((group) => (
                <section key={group.key} className="kb-history-group">
                  <h2 className="kb-history-day">{group.label}</h2>
                  <ul>
                    {group.entries.map((entry) => (
                      <HistoryRow
                        key={entry.id}
                        entry={entry}
                        selectMode={selectMode}
                        selected={selected.has(entry.id)}
                        workspaceLabel={
                          scope === 'all' ? workspaceName.get(entry.workspace_id) : undefined
                        }
                        onOpen={() => navigate(entry.url)}
                        onAuxOpen={() => {
                          if (activeWorkspaceId) {
                            void commands
                              .createBackgroundTab(activeWorkspaceId, entry.url)
                              .catch(() => undefined)
                          }
                        }}
                        onToggle={() => toggle(entry.id)}
                        onDelete={() => deleteEntries.mutate([entry.id])}
                      />
                    ))}
                  </ul>
                </section>
              ))}
            </div>
          )}
        </div>
      </div>

      <ConfirmDialog
        open={confirmClear}
        title={
          view === 'history'
            ? `Clear history in ${clearLabel}?`
            : `Clear recently closed tabs in ${clearLabel}?`
        }
        message={
          view === 'history'
            ? `This permanently removes ${shown} ${
                shown === 1 ? 'entry' : 'entries'
              } from ${clearLabel} and can't be undone.`
            : `${shown} ${shown === 1 ? 'tab' : 'tabs'} in ${clearLabel} can no longer be reopened afterwards.`
        }
        confirmLabel="Clear all"
        danger
        onConfirm={() => {
          clearScope.mutate()
          setConfirmClear(false)
        }}
        onCancel={() => setConfirmClear(false)}
      />
    </div>
  )
}

interface ViewTabProps {
  /** This page's id prefix. */
  ids: string
  view: View
  current: View
  onSelect: (view: View) => void
  children: React.ReactNode
}

function ViewTab({ ids, view, current, onSelect, children }: ViewTabProps) {
  return (
    <button
      type="button"
      role="tab"
      id={`${ids}-tab-${view}`}
      aria-controls={view === current ? `${ids}-panel-${view}` : undefined}
      aria-selected={view === current}
      className={cx('kb-history-view', { 'is-on': view === current })}
      onClick={() => onSelect(view)}
    >
      {children}
    </button>
  )
}

interface HistoryRowProps {
  entry: HistoryEntry
  selectMode: boolean
  selected: boolean
  /** Workspace name shown in the all-workspaces view. */
  workspaceLabel?: string
  onOpen: () => void
  /** Middle-click: open in a background tab. */
  onAuxOpen: () => void
  onToggle: () => void
  onDelete: () => void
}

function HistoryRow({
  entry,
  selectMode,
  selected,
  workspaceLabel,
  onOpen,
  onAuxOpen,
  onToggle,
  onDelete,
}: HistoryRowProps) {
  const host = hostOf(entry.url)
  const title = entry.title.trim() || host || entry.url
  return (
    <li className={cx('kb-history-row', { 'is-selected': selectMode && selected })}>
      <button
        type="button"
        className="kb-history-link"
        data-menu-url={entry.url}
        data-menu-label={title}
        onClick={selectMode ? onToggle : onOpen}
        onMouseDown={(e) => {
          if (e.button === 1) e.preventDefault()
        }}
        onAuxClick={(e) => {
          if (e.button !== 1 || selectMode) return
          e.preventDefault()
          onAuxOpen()
        }}
        title={entry.url}
      >
        {selectMode ? (
          <span className={cx('kb-history-check', { 'is-on': selected })} aria-hidden>
            {selected ? '✓' : ''}
          </span>
        ) : null}
        <span className="kb-history-time">{formatTime(entry.visited_at)}</span>
        <span className="kb-history-url">{host}</span>
        <span className="kb-history-title">{title}</span>
        {workspaceLabel ? <span className="kb-history-ws">{workspaceLabel}</span> : null}
      </button>
      {selectMode ? null : (
        <button
          type="button"
          className="kb-history-del"
          aria-label="Remove from history"
          title="Remove from history"
          onClick={onDelete}
        >
          <XIcon />
        </button>
      )}
    </li>
  )
}

function useDebounce<T>(value: T, delay: number): T {
  const [debounced, setDebounced] = useState(value)
  useEffect(() => {
    const id = setTimeout(() => setDebounced(value), delay)
    return () => clearTimeout(id)
  }, [value, delay])
  return debounced
}
