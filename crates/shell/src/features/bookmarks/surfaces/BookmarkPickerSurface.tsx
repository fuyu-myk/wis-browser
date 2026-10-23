/**
 * Folder picker for a just-added bookmark, hosted by the `overlay-bmpicker`
 * webview. Choosing a folder moves the bookmark there and persists the
 * choice as the default for the next bookmark.
 */

import { useCallback, useEffect, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { BookmarkFolder } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { bookmarkFoldersKey, bookmarksKey } from '@/features/bookmarks/lib/bookmarkKeys'
import {
  BMPICKER_CLOSE_EVENT,
  BMPICKER_OPEN_EVENT,
  lastFolderKey,
  type BmPickerOpen,
} from '@/features/bookmarks/lib/bookmarkMenu'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'
import { menusGlass } from '@/lib/glass'
import { useQueryClient } from '@tanstack/react-query'

export function BookmarkPickerSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)
  const glassMenus = menusGlass(state?.settings)
  const qc = useQueryClient()

  const [req, setReq] = useState<BmPickerOpen | null>(null)
  const [folders, setFolders] = useState<BookmarkFolder[]>([])
  const [selected, setSelected] = useState<number | null>(null)
  const [adding, setAdding] = useState(false)
  const [newName, setNewName] = useState('')

  const close = useCallback(() => {
    setReq(null)
    setAdding(false)
    setNewName('')
    void commands.hideOverlay('bmpicker').catch(() => undefined)
    void emit(BMPICKER_CLOSE_EVENT).catch(() => undefined)
  }, [])

  useEffect(() => {
    let cancelled = false
    let unlisten: UnlistenFn | null = null
    void listen<BmPickerOpen>(BMPICKER_OPEN_EVENT, (e) => {
      const payload = e.payload
      setReq(payload)
      setSelected(payload.currentFolderId)
      void commands
        .listBookmarkFolders(payload.workspaceId)
        .then((f) => setFolders(f))
        .catch(() => setFolders([]))
      void commands.focusSelf().catch(() => undefined)
    }).then((fn) => {
      if (cancelled) fn()
      else {
        unlisten = fn
        void emit(`${BMPICKER_OPEN_EVENT}-ready`)
      }
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') close()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [close])

  const choose = (folderId: number | null) => {
    if (!req) return
    setSelected(folderId)
    void commands.moveBookmark(req.bookmarkId, folderId).catch(() => undefined)
    try {
      localStorage.setItem(
        lastFolderKey(req.workspaceId),
        folderId == null ? 'unfiled' : String(folderId),
      )
    } catch {
      /* ignore storage errors */
    }
    void qc.invalidateQueries({ queryKey: bookmarksKey(req.workspaceId) })
    close()
  }

  const submitNewFolder = () => {
    const name = newName.trim()
    if (!name || !req) {
      setAdding(false)
      setNewName('')
      return
    }
    void commands
      .createBookmarkFolder(req.workspaceId, name)
      .then((folder) => {
        void qc.invalidateQueries({ queryKey: bookmarkFoldersKey(req.workspaceId) })
        choose(folder.id)
      })
      .catch(() => undefined)
    setNewName('')
    setAdding(false)
  }

  if (!req) return null

  return (
    <div className="kb-vaultmenu-backdrop" onMouseDown={close} role="presentation">
      <div
        className={cx('kb-vaultmenu', 'kb-bmpicker', { 'glass-menus': glassMenus })}
        role="menu"
        style={{ left: req.x, top: req.y, width: req.width }}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="kb-bmpicker-label">Save to</div>
        <button
          type="button"
          className={cx('kb-vaultmenu-row', { 'is-selected': selected == null })}
          role="menuitemradio"
          aria-checked={selected == null}
          onClick={() => choose(null)}
        >
          <span className="kb-bmpicker-name">Unfiled</span>
          {selected == null && <CheckGlyph />}
        </button>
        {folders.map((f) => (
          <button
            key={f.id}
            type="button"
            className={cx('kb-vaultmenu-row', { 'is-selected': selected === f.id })}
            role="menuitemradio"
            aria-checked={selected === f.id}
            onClick={() => choose(f.id)}
          >
            <span className="kb-bmpicker-name">{f.name}</span>
            {selected === f.id && <CheckGlyph />}
          </button>
        ))}
        <div className="kb-vaultmenu-sep" />
        {adding ? (
          <input
            className="kb-bmmenu-input"
            autoFocus
            placeholder="New folder name"
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') submitNewFolder()
              else if (e.key === 'Escape') {
                setAdding(false)
                setNewName('')
              }
            }}
            onBlur={submitNewFolder}
          />
        ) : (
          <button
            type="button"
            className="kb-vaultmenu-row"
            role="menuitem"
            onClick={() => setAdding(true)}
          >
            <span className="kb-bmpicker-name kb-bmpicker-new">+ New folder</span>
          </button>
        )}
      </div>
    </div>
  )
}

function CheckGlyph() {
  return (
    <svg
      className="kb-bmpicker-check"
      width={14}
      height={14}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2.4}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <polyline points="20 6 9 17 4 12" />
    </svg>
  )
}
