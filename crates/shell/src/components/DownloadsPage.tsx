/**
 * Full downloads page (`about:downloads`), modeled on the history page:
 * searchable, with a select mode (checkboxes + select all) whose delete asks
 * for confirmation. Clicking a completed row opens the file; a row whose file
 * is gone shows a moved-or-deleted notice instead.
 */

import { useEffect, useMemo, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { DownloadId, DownloadItem } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { FileIcon, SearchIcon, XIcon } from '@/lib/icons'
import {
  downloadHost,
  formatBytes,
  isInProgress,
  matchesDownloadSearch,
  DOWNLOADS_KEY,
} from '@/lib/downloads'
import { useDownloads } from '@/hooks/useDownloads'
import { ConfirmDialog } from '@/components/ConfirmDialog'

export function DownloadsPage() {
  const { downloads } = useDownloads()
  const qc = useQueryClient()

  const [search, setSearch] = useState('')
  const [selectMode, setSelectMode] = useState(false)
  const [selected, setSelected] = useState<Set<DownloadId>>(new Set())
  const [confirmDelete, setConfirmDelete] = useState(false)
  const [confirmRemove, setConfirmRemove] = useState<DownloadItem | null>(null)
  const [missing, setMissing] = useState<Set<DownloadId>>(new Set())

  const entries = useMemo(
    () => downloads.filter((d) => matchesDownloadSearch(d, search)),
    [downloads, search],
  )
  // Only terminal rows are deletable (no cancel API for in-flight ones).
  const deletable = useMemo(() => entries.filter((d) => !isInProgress(d)), [entries])

  const refresh = () => qc.invalidateQueries({ queryKey: DOWNLOADS_KEY })

  const removeMany = useMutation({
    mutationFn: (ids: DownloadId[]) => commands.removeDownloads(ids),
    onSuccess: refresh,
  })
  const removeOne = useMutation({
    mutationFn: (id: DownloadId) => commands.removeDownload(id),
    onSuccess: refresh,
  })

  const exitSelect = () => {
    setSelectMode(false)
    setSelected(new Set())
  }

  useEffect(() => {
    if (!selectMode) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        exitSelect()
      }
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [selectMode])

  // A search change can hide selected rows; drop the selection.
  useEffect(() => {
    setSelected(new Set())
  }, [search])

  const toggle = (id: DownloadId) => {
    setSelected((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }
  const allSelected = deletable.length > 0 && selected.size === deletable.length
  const toggleAll = () => {
    setSelected(allSelected ? new Set() : new Set(deletable.map((d) => d.id)))
  }

  const openFile = (item: DownloadItem) => {
    if (item.status !== 'completed') return
    void commands
      .openDownload(item.id)
      .then((found) => {
        if (!found) setMissing((prev) => new Set(prev).add(item.id))
      })
      .catch(() => setMissing((prev) => new Set(prev).add(item.id)))
  }

  const revealFile = (item: DownloadItem) => {
    void commands
      .revealDownload(item.id)
      .then((found) => {
        if (!found) setMissing((prev) => new Set(prev).add(item.id))
      })
      .catch(() => setMissing((prev) => new Set(prev).add(item.id)))
  }

  const groups = useMemo(() => groupByDay(entries), [entries])

  return (
    <div className="kb-history-page kb-dl-page">
      <div className="inner">
        <header className="kb-history-head">
          <div className="kb-history-head-row">
            <h1>Downloads</h1>
            {selectMode ? (
              <div className="kb-history-actions">
                <span className="kb-history-count">{selected.size} selected</span>
                <button
                  type="button"
                  className="kb-history-btn"
                  onClick={toggleAll}
                  disabled={deletable.length === 0}
                >
                  {allSelected ? 'Deselect all' : 'Select all'}
                </button>
                <button
                  type="button"
                  className="kb-history-btn is-danger"
                  onClick={() => setConfirmDelete(true)}
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
                  disabled={deletable.length === 0}
                >
                  Select
                </button>
              </div>
            )}
          </div>
          <div className="kb-history-head-row">
            <div className="kb-history-search">
              <SearchIcon />
              <input
                type="text"
                placeholder="Search downloads"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                autoFocus
                spellCheck={false}
              />
            </div>
          </div>
        </header>

        {entries.length === 0 ? (
          <p className="kb-history-empty">
            {search.trim() ? 'No matching downloads.' : 'No downloads yet.'}
          </p>
        ) : (
          <div className="kb-history-groups">
            {groups.map((group) => (
              <section key={group.key} className="kb-history-group">
                <h2 className="kb-history-day">{group.label}</h2>
                <ul>
                  {group.entries.map((item) => (
                    <DownloadPageRow
                      key={item.id}
                      item={item}
                      selectMode={selectMode}
                      selected={selected.has(item.id)}
                      missing={missing.has(item.id)}
                      onOpen={() => openFile(item)}
                      onReveal={() => revealFile(item)}
                      onToggle={() => toggle(item.id)}
                      onRemove={() => setConfirmRemove(item)}
                    />
                  ))}
                </ul>
              </section>
            ))}
          </div>
        )}
      </div>

      <ConfirmDialog
        open={confirmDelete}
        title={`Delete ${selected.size} ${selected.size === 1 ? 'download' : 'downloads'} from history?`}
        message="The downloaded files stay on disk; only the history entries are removed. This can't be undone."
        confirmLabel="Delete"
        danger
        onConfirm={() => {
          removeMany.mutate([...selected])
          setConfirmDelete(false)
          exitSelect()
        }}
        onCancel={() => setConfirmDelete(false)}
      />

      <ConfirmDialog
        open={confirmRemove != null}
        title={`Delete "${confirmRemove?.filename ?? ''}" from history?`}
        message="The downloaded file stays on disk; only the history entry is removed. This can't be undone."
        confirmLabel="Delete"
        danger
        onConfirm={() => {
          if (confirmRemove) removeOne.mutate(confirmRemove.id)
          setConfirmRemove(null)
        }}
        onCancel={() => setConfirmRemove(null)}
      />
    </div>
  )
}

interface DownloadPageRowProps {
  item: DownloadItem
  selectMode: boolean
  selected: boolean
  missing: boolean
  onOpen: () => void
  onReveal: () => void
  onToggle: () => void
  onRemove: () => void
}

function DownloadPageRow({
  item,
  selectMode,
  selected,
  missing,
  onOpen,
  onReveal,
  onToggle,
  onRemove,
}: DownloadPageRowProps) {
  const inFlight = isInProgress(item)
  const selectable = selectMode && !inFlight
  const sub = missing
    ? 'File may have been moved or deleted'
    : item.status === 'failed'
      ? 'Failed'
      : item.status === 'cancelled'
        ? 'Cancelled'
        : `${formatBytes(item.receivedBytes)} · ${downloadHost(item.url)}`
  return (
    <li className={cx('kb-history-row', 'kb-dl-row', { 'is-selected': selectable && selected })}>
      <button
        type="button"
        className="kb-history-link"
        onClick={selectable ? onToggle : onOpen}
        disabled={!selectable && item.status !== 'completed'}
        title={missing ? undefined : item.path}
      >
        {selectable ? (
          <span className={cx('kb-history-check', { 'is-on': selected })} aria-hidden>
            {selected ? '✓' : ''}
          </span>
        ) : null}
        <span className="kb-history-time">{formatTime(item.startedAtMs)}</span>
        <span className="kb-dl-fileicon" aria-hidden>
          <FileIcon />
        </span>
        <span className="kb-history-title">{item.filename}</span>
        <span className={cx('kb-dl-pagesub', { 'is-error': missing || item.status === 'failed' })}>
          {inFlight ? `Downloading, ${formatBytes(item.receivedBytes)} so far` : sub}
        </span>
        {inFlight ? <span className="kb-dl-bar" aria-hidden /> : null}
      </button>
      {selectMode || inFlight ? null : (
        <>
          <button
            type="button"
            className="kb-history-btn kb-dl-reveal"
            onClick={onReveal}
            disabled={item.status !== 'completed'}
          >
            Show in Finder
          </button>
          <button
            type="button"
            className="kb-history-del"
            aria-label="Remove from downloads"
            title="Remove from downloads"
            onClick={onRemove}
          >
            <XIcon />
          </button>
        </>
      )}
    </li>
  )
}

// -- helpers ----------------------------------------------------------------

interface DayGroup {
  key: string
  label: string
  entries: DownloadItem[]
}

/** Group already-sorted (newest-first) items by local calendar day. */
function groupByDay(items: DownloadItem[]): DayGroup[] {
  const groups: DayGroup[] = []
  let current: DayGroup | null = null
  for (const item of items) {
    const date = new Date(item.startedAtMs)
    const key = dayKey(date)
    if (!current || current.key !== key) {
      current = { key, label: dayLabel(date), entries: [] }
      groups.push(current)
    }
    current.entries.push(item)
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

function formatTime(ms: number): string {
  return new Date(ms).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' })
}
