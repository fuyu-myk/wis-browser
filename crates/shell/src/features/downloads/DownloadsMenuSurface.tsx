/**
 * Downloads popup hosted by the `overlay-downloads` webview. The card aligns
 * toward the sidebar/rail side and shows the latest downloads: in-flight rows
 * with live progress, completed rows that open the file on click (with a
 * moved-or-deleted notice when the file is gone), and a footer button to the
 * full downloads page.
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { DownloadId, DownloadItem } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { FileIcon, XIcon } from '@/lib/icons'
import { currentPlatform } from '@/lib/platform'
import { isChromeUrl, DOWNLOADS_URL } from '@/lib/url'
import {
  downloadHost,
  isInProgress,
  statusLabel,
  DOWNLOADS_CLOSED_EVENT,
  DOWNLOADS_KEY,
  DOWNLOADS_SHOWN_EVENT,
  POPUP_LIMIT,
} from '@/features/downloads/downloads'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useDownloads } from '@/features/downloads/useDownloads'
import { useThemeApply } from '@/hooks/useThemeApply'
import { useSurfaceFocus } from '@/hooks/useSurfaceFocus'
import { menusGlass } from '@/lib/glass'
import { ConfirmDialog } from '@/components/ConfirmDialog'

export function DownloadsMenuSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)
  const { downloads } = useDownloads()
  const qc = useQueryClient()

  const glassMenus = menusGlass(state?.settings)
  const layout = state?.settings.layout ?? 'sidebar_left'
  const railSide = state?.settings.top_tabs_rail_side ?? 'left'
  const side: 'left' | 'right' =
    layout === 'sidebar_right' ? 'right' : layout === 'top_tabs' ? railSide : 'left'

  const activeWorkspace = state?.workspaces.find((w) => w.id === state.active_workspace_id)
  const activeTab =
    activeWorkspace?.active_tab_id != null
      ? (state?.tabs.find((t) => t.id === activeWorkspace.active_tab_id) ?? null)
      : null

  // Monotonic reveal sequence (see reveal below); closes bump it too.
  const revealSeqRef = useRef(0)
  // Per-row "file may have been moved or deleted" notice, cleared on refetch.
  const [missing, setMissing] = useState<Set<DownloadId>>(new Set())
  const [confirmRemove, setConfirmRemove] = useState<DownloadItem | null>(null)
  // Ref'd so the Esc handler can check it without re-binding the listener.
  const confirmRemoveRef = useRef<DownloadItem | null>(null)
  confirmRemoveRef.current = confirmRemove

  const close = useCallback(() => {
    setConfirmRemove(null)
    revealSeqRef.current += 1
    setVisible(false)
    void commands.hideOverlay('downloads').catch(() => undefined)
    void emit(DOWNLOADS_CLOSED_EVENT).catch(() => undefined)
  }, [])

  const [visible, setVisible] = useState(false)
  const [openNonce, setOpenNonce] = useState(0)
  useSurfaceFocus(visible && openNonce)
  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    const reveal = () => {
      // Sequence token: a close or a newer reveal supersedes this one.
      const seq = ++revealSeqRef.current
      void qc
        .refetchQueries({ queryKey: DOWNLOADS_KEY })
        .catch(() => undefined)
        .finally(() => {
          if (cancelled || seq !== revealSeqRef.current) return
          setOpenNonce((n) => n + 1)
          setVisible(true)
        })
    }
    void listen<void>(DOWNLOADS_SHOWN_EVENT, reveal).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    reveal()
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [qc])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      // The confirm dialog's own Esc handler cancels it; don't also close.
      if (confirmRemoveRef.current) return
      close()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [close])

  const removeOne = useMutation({
    mutationFn: (id: DownloadId) => commands.removeDownload(id),
    onSuccess: () => void qc.invalidateQueries({ queryKey: DOWNLOADS_KEY }),
  })

  const cancelOne = useMutation({
    mutationFn: (id: DownloadId) => commands.cancelDownload(id),
    onSuccess: () => void qc.invalidateQueries({ queryKey: DOWNLOADS_KEY }),
  })

  const openFile = (item: DownloadItem) => {
    if (item.status !== 'completed') return
    void commands
      .openDownload(item.id)
      .then((found) => {
        if (found) close()
        else setMissing((prev) => new Set(prev).add(item.id))
      })
      .catch(() => setMissing((prev) => new Set(prev).add(item.id)))
  }

  const openFullPage = () => {
    if (activeTab && isChromeUrl(activeTab.url)) {
      void commands.navigateTab(activeTab.id, DOWNLOADS_URL).catch(() => undefined)
    } else if (activeWorkspace) {
      void commands.createTab(activeWorkspace.id, DOWNLOADS_URL).catch(() => undefined)
    }
    close()
  }

  const latest = downloads.slice(0, POPUP_LIMIT)

  if (!visible) return null

  return (
    <div className={cx('kb-dlmenu-backdrop', { 'glass-menus': glassMenus })} onMouseDown={close} role="presentation">
      <div
        key={openNonce}
        className={cx('kb-dlmenu', `side-${side}`, {
          'glass-menus': glassMenus,
          'is-top-tabs': layout === 'top_tabs',
        })}
        role="dialog"
        aria-label="Downloads"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="kb-dlmenu-head">
          <span className="kb-dlmenu-title">Downloads</span>
        </div>

        <div className="kb-dlmenu-scroll">
          {latest.length === 0 ? (
            <p className="kb-dlmenu-empty">No downloads yet.</p>
          ) : (
            <ul className="kb-dlmenu-list">
              {latest.map((item, i) => (
                <DownloadRow
                  key={item.id}
                  item={item}
                  index={i}
                  missing={missing.has(item.id)}
                  onOpen={() => openFile(item)}
                  onRemove={() => setConfirmRemove(item)}
                  onCancel={
                    currentPlatform() === 'macos'
                      ? () => cancelOne.mutate(item.id)
                      : undefined
                  }
                />
              ))}
            </ul>
          )}
        </div>

        <button type="button" className="kb-dlmenu-all" onClick={openFullPage}>
          View all downloads
        </button>
      </div>

      {/* Stop mousedown from reaching the popup backdrop (which closes the popup). */}
      <div onMouseDown={(e) => e.stopPropagation()} role="presentation">
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
    </div>
  )
}

interface DownloadRowProps {
  item: DownloadItem
  /** Position in the visible list, staggering the slide-in entrance. */
  index: number
  missing: boolean
  onOpen: () => void
  onRemove: () => void
  /** Cancel the in-flight engine download; absent where unsupported. */
  onCancel?: () => void
}

function DownloadRow({ item, index, missing, onOpen, onRemove, onCancel }: DownloadRowProps) {
  const inFlight = isInProgress(item)
  return (
    <li
      className={cx('kb-dlmenu-row', { 'is-missing': missing })}
      style={{ animationDelay: `${Math.min(index, 8) * 24}ms` }}
    >
      <button
        type="button"
        className="kb-dlmenu-tile"
        onClick={onOpen}
        disabled={item.status !== 'completed'}
        title={missing ? undefined : item.path}
      >
        <span className="kb-dl-fileicon" aria-hidden>
          <FileIcon />
        </span>
        <span className="kb-dl-meta">
          <span className="kb-dl-name">{item.filename}</span>
          <span className={cx('kb-dl-sub', { 'is-error': missing || item.status === 'failed' })}>
            {missing ? 'File may have been moved or deleted' : statusLabel(item)}
            {missing ? null : <span className="kb-dl-host"> · {downloadHost(item.url)}</span>}
          </span>
          {inFlight ? <span className="kb-dl-bar" aria-hidden /> : null}
        </span>
      </button>
      {inFlight ? (
        onCancel ? (
          <button
            type="button"
            className="kb-dlmenu-del"
            aria-label="Cancel download"
            title="Cancel download"
            onClick={onCancel}
          >
            <XIcon />
          </button>
        ) : null
      ) : (
        <button
          type="button"
          className="kb-dlmenu-del"
          aria-label="Remove from downloads"
          title="Remove from downloads"
          onClick={onRemove}
        >
          <XIcon />
        </button>
      )}
    </li>
  )
}
