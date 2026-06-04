import { useEffect, useMemo, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { HistoryEntry, Tab, WorkspaceId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { SearchIcon, XIcon } from '@/lib/icons'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useNavigateOrCreateTab } from '@/hooks/useNavigateOrCreateTab'
import { ConfirmDialog } from '@/components/ConfirmDialog'

const HISTORY_KEY = ['history'] as const
const SEARCH_DEBOUNCE_MS = 250
const PAGE_LIMIT = 500

type Scope = 'workspace' | 'all'

interface HistoryPageProps {
  /** The tab currently showing this page (navigated in place on click). */
  activeTab: Tab | null
}

export function HistoryPage({ activeTab }: HistoryPageProps) {
  const { data: state } = useBrowserState()
  const qc = useQueryClient()
  const activeWorkspaceId = state?.active_workspace_id ?? null

  const [search, setSearch] = useState('')
  const debouncedSearch = useDebounce(search, SEARCH_DEBOUNCE_MS)
  const [scope, setScope] = useState<Scope>('workspace')

  const [selectMode, setSelectMode] = useState(false)
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const [confirmClear, setConfirmClear] = useState(false)

  const navigate = useNavigateOrCreateTab(activeWorkspaceId, activeTab)

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
  })

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
    mutationFn: () =>
      commands.clearHistory(scope === 'workspace' ? activeWorkspaceId ?? undefined : undefined),
    onSuccess: refresh,
  })

  // A scope change can hide selected rows; drop the selection to avoid acting
  // on entries no longer visible.
  useEffect(() => {
    setSelected(new Set())
    setSelectMode(false)
  }, [scope])

  const groups = useMemo(() => groupByDay(entries), [entries])

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

  const clearLabel = scope === 'workspace' ? 'this workspace' : 'all workspaces'

  return (
    <div className="kb-history-page">
      <div className="inner">
        <header className="kb-history-head">
          <h1>History</h1>
          <div className="kb-history-scope" role="tablist" aria-label="History scope">
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
          <div className="kb-history-search">
            <SearchIcon />
            <input
              type="text"
              placeholder="Search history"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              autoFocus
              spellCheck={false}
            />
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
              <button
                type="button"
                className="kb-history-btn"
                onClick={() => setSelectMode(true)}
                disabled={entries.length === 0}
              >
                Select
              </button>
              <button
                type="button"
                className="kb-history-btn is-danger"
                onClick={() => setConfirmClear(true)}
                disabled={entries.length === 0 || clearScope.isPending}
              >
                Clear all
              </button>
            </div>
          )}
        </header>

        {isLoading ? null : entries.length === 0 ? (
          <p className="kb-history-empty">
            {debouncedSearch ? 'No matching history.' : 'No history yet.'}
          </p>
        ) : (
          <div className="kb-history-groups">
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

      <ConfirmDialog
        open={confirmClear}
        title={`Clear history in ${clearLabel}?`}
        message={`This permanently removes ${entries.length} ${
          entries.length === 1 ? 'entry' : 'entries'
        } from ${clearLabel} and can't be undone.`}
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

interface HistoryRowProps {
  entry: HistoryEntry
  selectMode: boolean
  selected: boolean
  /** Workspace name shown in the all-workspaces view. */
  workspaceLabel?: string
  onOpen: () => void
  onToggle: () => void
  onDelete: () => void
}

function HistoryRow({
  entry,
  selectMode,
  selected,
  workspaceLabel,
  onOpen,
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
        onClick={selectMode ? onToggle : onOpen}
        title={entry.url}
      >
        {selectMode ? (
          <span className={cx('kb-history-check', { 'is-on': selected })} aria-hidden>
            {selected ? '✓' : ''}
          </span>
        ) : null}
        <span className="kb-history-time">{formatTime(entry.visited_at)}</span>
        <span className="kb-history-title">{title}</span>
        <span className="kb-history-url">{host}</span>
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

// -- helpers ----------------------------------------------------------------

interface DayGroup {
  key: string
  label: string
  entries: HistoryEntry[]
}

/** Group already-sorted (newest-first) entries by local calendar day. */
function groupByDay(entries: HistoryEntry[]): DayGroup[] {
  const groups: DayGroup[] = []
  let current: DayGroup | null = null
  for (const entry of entries) {
    const date = new Date(entry.visited_at)
    const key = dayKey(date)
    if (!current || current.key !== key) {
      current = { key, label: dayLabel(date), entries: [] }
      groups.push(current)
    }
    current.entries.push(entry)
  }
  return groups
}

function dayKey(date: Date): string {
  return `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`
}

function dayLabel(date: Date): string {
  const today = new Date()
  const yesterday = new Date()
  yesterday.setDate(today.getDate() - 1)
  if (dayKey(date) === dayKey(today)) return 'Today'
  if (dayKey(date) === dayKey(yesterday)) return 'Yesterday'
  return date.toLocaleDateString(undefined, {
    weekday: 'long',
    month: 'long',
    day: 'numeric',
    year: date.getFullYear() === today.getFullYear() ? undefined : 'numeric',
  })
}

function formatTime(iso: string): string {
  return new Date(iso).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })
}

function hostOf(url: string): string {
  try {
    return new URL(url).host.replace(/^www\./, '')
  } catch {
    return url
  }
}

function useDebounce<T>(value: T, delay: number): T {
  const [debounced, setDebounced] = useState(value)
  useEffect(() => {
    const id = setTimeout(() => setDebounced(value), delay)
    return () => clearTimeout(id)
  }, [value, delay])
  return debounced
}
