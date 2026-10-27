/**
 * Bookmarks popup hosted by the `overlay-bookmarks` webview. The card aligns toward
 * the sidebar side. Shows unfiled bookmarks as favicon bubbles, then folders as a
 * list that expands to indented favicons. Folder create/rename/delete are inline.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { MouseEvent as ReactMouseEvent } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { Bookmark, BookmarkFolder, WorkspaceId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { PlusIcon, XIcon } from '@/lib/icons'
import { isChromeUrl } from '@/lib/url'
import { bookmarkFoldersKey, bookmarksKey } from '@/features/bookmarks/lib/bookmarkKeys'
import { BOOKMARKS_CLOSED_EVENT, PIN_CAP } from '@/features/bookmarks/lib/bookmarkMenu'
import { useBookmarkSync } from '@/features/bookmarks/hooks/useBookmarkSync'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'
import { menusGlass } from '@/lib/glass'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { BookmarkFavicon } from '@/features/bookmarks/components/BookmarkFavicon'
import { moveFocusByArrow } from '@/lib/arrowNav'

export function BookmarksMenuSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)
  useBookmarkSync()
  const qc = useQueryClient()

  const glassMenus = menusGlass(state?.settings)
  const layout = state?.settings.layout ?? 'sidebar_left'
  const railSide = state?.settings.top_tabs_rail_side ?? 'left'
  const side: 'left' | 'right' =
    layout === 'sidebar_right' ? 'right' : layout === 'top_tabs' ? railSide : 'left'

  const activeWorkspace = state?.workspaces.find((w) => w.id === state.active_workspace_id)
  const workspaceId: WorkspaceId | null = activeWorkspace?.id ?? null
  const activeTab =
    activeWorkspace?.active_tab_id != null
      ? (state?.tabs.find((t) => t.id === activeWorkspace.active_tab_id) ?? null)
      : null

  const folders = useQuery({
    queryKey: workspaceId ? bookmarkFoldersKey(workspaceId) : ['bookmark-folders', 'none'],
    queryFn: () => commands.listBookmarkFolders(workspaceId as WorkspaceId),
    enabled: !!workspaceId,
  })
  const bookmarks = useQuery({
    queryKey: workspaceId ? bookmarksKey(workspaceId) : ['bookmarks', 'none'],
    queryFn: () => commands.listBookmarks(workspaceId as WorkspaceId),
    enabled: !!workspaceId,
  })

  const invalidate = () => {
    if (!workspaceId) return
    void qc.invalidateQueries({ queryKey: bookmarkFoldersKey(workspaceId) })
    void qc.invalidateQueries({ queryKey: bookmarksKey(workspaceId) })
  }

  // Apply an optimistic cache patch that survives concurrent refetches.
  const commit = (patch: () => void) => {
    void (async () => {
      if (workspaceId) {
        await qc.cancelQueries({ queryKey: bookmarksKey(workspaceId) })
        await qc.cancelQueries({ queryKey: bookmarkFoldersKey(workspaceId) })
      }
      patch()
      invalidate()
    })()
  }
  const patchBookmarks = (fn: (list: Bookmark[]) => Bookmark[]) => {
    if (!workspaceId) return
    qc.setQueryData<Bookmark[]>(bookmarksKey(workspaceId), (old) => fn(old ?? []))
  }
  const patchFolders = (fn: (list: BookmarkFolder[]) => BookmarkFolder[]) => {
    if (!workspaceId) return
    qc.setQueryData<BookmarkFolder[]>(bookmarkFoldersKey(workspaceId), (old) => fn(old ?? []))
  }

  const createFolder = useMutation({
    mutationFn: (name: string) => commands.createBookmarkFolder(workspaceId as WorkspaceId, name),
    onSuccess: (folder) => commit(() => patchFolders((list) => [...list, folder])),
  })
  const renameFolder = useMutation({
    mutationFn: ({ id, name }: { id: number; name: string }) =>
      commands.renameBookmarkFolder(id, name),
    onSuccess: (_void, { id, name }) =>
      commit(() => patchFolders((list) => list.map((f) => (f.id === id ? { ...f, name } : f)))),
  })
  const reorderFolders = useMutation({
    mutationFn: (ids: number[]) => commands.reorderBookmarkFolders(ids),
    onSuccess: (_void, ids) =>
      commit(() =>
        patchFolders((list) => {
          const map = new Map(list.map((f) => [f.id, f]))
          return ids
            .map((id, i) => {
              const f = map.get(id)
              return f ? { ...f, position: i } : null
            })
            .filter((f): f is BookmarkFolder => f != null)
        }),
      ),
  })
  const deleteFolder = useMutation({
    mutationFn: (id: number) => commands.deleteBookmarkFolder(id),
    onSuccess: (_void, id) =>
      commit(() => {
        patchFolders((list) => list.filter((f) => f.id !== id))
        patchBookmarks((list) =>
          list.map((b) => (b.folder_id === id ? { ...b, folder_id: null } : b)),
        )
      }),
  })
  const deleteBookmark = useMutation({
    mutationFn: (id: number) => commands.deleteBookmark(id),
    onSuccess: (_void, id) => commit(() => patchBookmarks((list) => list.filter((b) => b.id !== id))),
  })
  const moveBookmark = useMutation({
    mutationFn: ({ id, folderId }: { id: number; folderId: number | null }) =>
      commands.moveBookmark(id, folderId),
    onSuccess: (_void, { id, folderId }) =>
      commit(() =>
        patchBookmarks((list) => list.map((b) => (b.id === id ? { ...b, folder_id: folderId } : b))),
      ),
  })
  const setPinned = useMutation({
    mutationFn: ({ id, pinned }: { id: number; pinned: boolean }) =>
      commands.setBookmarkPinned(id, pinned),
    onSuccess: (_void, { id, pinned }) =>
      commit(() =>
        patchBookmarks((list) => {
          // Pinning places it at the front of the row.
          const frontPos =
            list.reduce((m, b) => (b.pinned ? Math.min(m, b.pin_position) : m), 0) - 1
          return list.map((b) =>
            b.id === id ? { ...b, pinned, pin_position: pinned ? frontPos : b.pin_position } : b,
          )
        }),
      ),
  })
  const reorderPinned = useMutation({
    mutationFn: (ids: number[]) => commands.reorderPinnedBookmarks(ids),
    onSuccess: (_void, ids) =>
      commit(() => {
        const order = new Map(ids.map((id, i) => [id, i]))
        patchBookmarks((list) =>
          list.map((b) =>
            order.has(b.id) ? { ...b, pinned: true, pin_position: order.get(b.id) as number } : b,
          ),
        )
      }),
  })
  const reorderBookmarks = useMutation({
    mutationFn: (ids: number[]) => commands.reorderBookmarks(ids),
    onSuccess: (_void, ids) =>
      commit(() => {
        const order = new Map(ids.map((id, i) => [id, i]))
        patchBookmarks((list) =>
          list.map((b) => (order.has(b.id) ? { ...b, position: order.get(b.id) as number } : b)),
        )
      }),
  })

  // Drag-and-drop: a favicon can be dropped onto a folder (moves it in) or onto
  // the pinned bubble area / a specific bubble (pins/reorders it); a folder
  // header can be dragged over another folder to reorder. `dragKindRef`
  // distinguishes the two so a folder's drop handler does the right thing.
  const [dropFolder, setDropFolder] = useState<number | null>(null)
  const [dropBubble, setDropBubble] = useState<number | null>(null)
  const [reorderOver, setReorderOver] = useState<number | null>(null)
  const [dropUnfiled, setDropUnfiled] = useState(false)
  // Insertion indicator while reordering a favicon within a folder.
  const [dropRow, setDropRow] = useState<{ id: number; after: boolean } | null>(null)
  const dragIdRef = useRef<number | null>(null)
  const dragKindRef = useRef<'favicon' | 'folder' | null>(null)
  const dragFolderIdRef = useRef<number | null>(null)
  const draggedRef = useRef(false)

  const onFaviconDragStart = (e: React.DragEvent, id: number) => {
    dragIdRef.current = id
    dragFolderIdRef.current = null
    dragKindRef.current = 'favicon'
    draggedRef.current = true
    e.dataTransfer.effectAllowed = 'move'
    e.dataTransfer.setData('text/plain', String(id))
  }
  const onFaviconDragEnd = () => {
    dragIdRef.current = null
    dragKindRef.current = null
    setDropFolder(null)
    setDropBubble(null)
    // Clear the click-suppression flag on the next tick (after any click fires).
    setTimeout(() => {
      draggedRef.current = false
    }, 0)
  }
  const onFolderDragStart = (e: React.DragEvent, folderId: number) => {
    dragKindRef.current = 'folder'
    dragFolderIdRef.current = folderId
    dragIdRef.current = null
    e.dataTransfer.effectAllowed = 'move'
    e.dataTransfer.setData('text/plain', `folder:${folderId}`)
  }
  const onFolderDragEnd = () => {
    dragKindRef.current = null
    dragFolderIdRef.current = null
    setDropFolder(null)
    setReorderOver(null)
  }
  // Read the active drag from the transfer payload (set on dragstart), which is
  // authoritative.
  type DragInfo = { kind: 'favicon'; id: number } | { kind: 'folder'; id: number }
  const readDrag = (e: React.DragEvent): DragInfo | null => {
    const raw = e.dataTransfer.getData('text/plain')
    if (raw.startsWith('folder:')) {
      const id = Number(raw.slice('folder:'.length))
      if (Number.isFinite(id)) return { kind: 'folder', id }
    } else if (raw !== '') {
      const id = Number(raw)
      if (Number.isFinite(id)) return { kind: 'favicon', id }
    }
    if (dragKindRef.current === 'folder' && dragFolderIdRef.current != null) {
      return { kind: 'folder', id: dragFolderIdRef.current }
    }
    if (dragIdRef.current != null) return { kind: 'favicon', id: dragIdRef.current }
    return null
  }
  const clearDrag = () => {
    dragIdRef.current = null
    dragKindRef.current = null
    dragFolderIdRef.current = null
  }
  const onFolderDragOver = (e: React.DragEvent, folderId: number) => {
    // Always accept on a folder so it's a valid drop target (and the drop event
    // fires); the actual action is resolved on drop.
    e.preventDefault()
    e.dataTransfer.dropEffect = 'move'
    if (dragKindRef.current === 'folder') {
      // Reordering: highlight the hovered folder (unless it's the one dragged).
      setDropFolder(null)
      setReorderOver(dragFolderIdRef.current === folderId ? null : folderId)
    } else if (dropFolder !== folderId) {
      setDropFolder(folderId)
    }
  }
  const onFolderDragLeave = (folderId: number) => {
    setDropFolder((prev) => (prev === folderId ? null : prev))
    setReorderOver((prev) => (prev === folderId ? null : prev))
  }
  const onFolderDrop = (e: React.DragEvent, folderId: number) => {
    e.preventDefault()
    setDropFolder(null)
    setReorderOver(null)
    const drag = readDrag(e)
    clearDrag()
    if (!drag) return
    if (drag.kind === 'folder') {
      if (drag.id === folderId) return
      // Insert before/after the target depending on which half was dropped on.
      const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
      const after = e.clientY > rect.top + rect.height / 2
      const ids = folderList.map((f) => f.id).filter((id) => id !== drag.id)
      let at = ids.indexOf(folderId)
      if (at < 0) return
      if (after) at += 1
      ids.splice(at, 0, drag.id)
      reorderFolders.mutate(ids)
      return
    }
    const bm = allBookmarks.find((b) => b.id === drag.id)
    if (bm && bm.folder_id !== folderId) moveBookmark.mutate({ id: drag.id, folderId })
  }
  // Pinned bubble area: drop onto a bubble to insert/replace at that slot, or
  // anywhere in the area to pin at the end.
  const onBubbleDragOver = (e: React.DragEvent, index: number) => {
    e.preventDefault()
    e.stopPropagation()
    e.dataTransfer.dropEffect = 'move'
    if (dropBubble !== index) setDropBubble(index)
  }
  const onBubbleDrop = (e: React.DragEvent, index: number) => {
    e.preventDefault()
    e.stopPropagation()
    setDropBubble(null)
    const drag = readDrag(e)
    clearDrag()
    if (drag?.kind === 'favicon') pinAtSlot(drag.id, index)
  }
  const onBubbleAreaDragOver = (e: React.DragEvent) => {
    e.preventDefault()
    e.dataTransfer.dropEffect = 'move'
  }
  const onBubbleAreaDrop = (e: React.DragEvent) => {
    e.preventDefault()
    setDropBubble(null)
    const drag = readDrag(e)
    clearDrag()
    if (drag?.kind === 'favicon') pinAtEnd(drag.id)
  }
  // Unfiled area (below the folders): drop a favicon here to take it out of its
  // folder (move to unfiled). Pinned state is kept.
  const onUnfiledDragOver = (e: React.DragEvent) => {
    if (dragKindRef.current === 'folder') return
    e.preventDefault()
    e.dataTransfer.dropEffect = 'move'
    if (!dropUnfiled) setDropUnfiled(true)
  }
  const onUnfiledDragLeave = () => setDropUnfiled(false)
  const onUnfiledDrop = (e: React.DragEvent) => {
    e.preventDefault()
    setDropUnfiled(false)
    const drag = readDrag(e)
    clearDrag()
    if (drag?.kind !== 'favicon') return
    const bm = allBookmarks.find((b) => b.id === drag.id)
    if (!bm) return
    if (bm.folder_id != null) moveBookmark.mutate({ id: drag.id, folderId: null })
    if (bm.pinned) setPinned.mutate({ id: drag.id, pinned: false })
  }
  // Reorder favicons within a folder: drop a folder favicon onto another row in
  // the same folder.
  const onRowDragOver = (e: React.DragEvent, folderId: number, bookmarkId: number) => {
    if (dragKindRef.current === 'folder') return
    e.preventDefault()
    e.stopPropagation()
    e.dataTransfer.dropEffect = 'move'
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
    const after = e.clientY > rect.top + rect.height / 2
    setDropFolder(null)
    setDropRow((prev) =>
      prev && prev.id === bookmarkId && prev.after === after ? prev : { id: bookmarkId, after },
    )
  }
  const onRowDragLeave = (bookmarkId: number) => {
    setDropRow((prev) => (prev && prev.id === bookmarkId ? null : prev))
  }
  const onRowDrop = (e: React.DragEvent, folderId: number, bookmarkId: number) => {
    e.preventDefault()
    e.stopPropagation()
    setDropRow(null)
    setDropFolder(null)
    const drag = readDrag(e)
    clearDrag()
    if (drag?.kind !== 'favicon' || drag.id === bookmarkId) return
    const bm = allBookmarks.find((b) => b.id === drag.id)
    if (!bm) return
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
    const after = e.clientY > rect.top + rect.height / 2
    const ids = allBookmarks
      .filter((b) => b.folder_id === folderId && b.id !== drag.id)
      .sort((a, b) => a.position - b.position)
      .map((b) => b.id)
    let at = ids.indexOf(bookmarkId)
    if (at < 0) at = ids.length
    if (after) at += 1
    ids.splice(at, 0, drag.id)
    if (bm.folder_id !== folderId) moveBookmark.mutate({ id: drag.id, folderId })
    reorderBookmarks.mutate(ids)
  }

  const [openFolders, setOpenFolders] = useState<Set<number>>(new Set())
  const [adding, setAdding] = useState(false)
  const [newName, setNewName] = useState('')
  const [renaming, setRenaming] = useState<number | null>(null)
  const [renameValue, setRenameValue] = useState('')
  const [confirmDelete, setConfirmDelete] = useState<number | null>(null)
  // Right-click "assign to folder" menu, anchored at the cursor.
  const [ctx, setCtx] = useState<{ id: number; x: number; y: number } | null>(null)
  const ctxRef = useRef(ctx)
  ctxRef.current = ctx

  const openCtx = (e: React.MouseEvent, id: number) => {
    e.preventDefault()
    e.stopPropagation()
    setCtx({ id, x: e.clientX, y: e.clientY })
  }
  const assignCtx = (folderId: number | null) => {
    const c = ctxRef.current
    if (!c) return
    const bm = allBookmarks.find((b) => b.id === c.id)
    if (bm && bm.folder_id !== folderId) moveBookmark.mutate({ id: c.id, folderId })
    setCtx(null)
  }

  const folderList = folders.data ?? []
  const allBookmarks = useMemo(() => bookmarks.data ?? [], [bookmarks.data])
  // Pinned quick-access bubbles, ordered by pin_position. Pinning is independent
  // of folder membership, so a pinned bookmark also still appears in its folder.
  const pinned = useMemo(
    () =>
      allBookmarks
        .filter((b) => b.pinned)
        .sort((a, b) => a.pin_position - b.pin_position),
    [allBookmarks],
  )
  // Unfiled bookmarks that are not pinned show as a list below the folders.
  const restUnfiled = useMemo(
    () =>
      allBookmarks
        .filter((b) => b.folder_id == null && !b.pinned)
        .sort((a, b) => a.position - b.position),
    [allBookmarks],
  )
  const isPinned = (id: number) => allBookmarks.some((b) => b.id === id && b.pinned)

  // Drop a bookmark onto the pinned row at `index`. Folder membership is
  // untouched (pinning is independent).
  const pinAtSlot = (id: number, index: number) => {
    const ids = pinned.map((b) => b.id)
    if (ids.includes(id)) {
      const without = ids.filter((x) => x !== id)
      const at = Math.max(0, Math.min(index, without.length))
      without.splice(at, 0, id)
      reorderPinned.mutate(without)
      return
    }
    if (ids.length < PIN_CAP) {
      const at = Math.max(0, Math.min(index, ids.length))
      const next = [...ids]
      next.splice(at, 0, id)
      reorderPinned.mutate(next)
      return
    }
    const slot = Math.max(0, Math.min(index, ids.length - 1))
    const replaced = ids[slot]
    setPinned.mutate({ id: replaced, pinned: false })
    reorderPinned.mutate(ids.map((x) => (x === replaced ? id : x)))
  }
  // Drop a bookmark into the pinned area (not a specific bubble): append while
  // there's room; ignore when full (drop onto a bubble to replace instead).
  const pinAtEnd = (id: number) => {
    const ids = pinned.map((b) => b.id)
    if (ids.includes(id)) {
      reorderPinned.mutate([...ids.filter((x) => x !== id), id])
      return
    }
    if (ids.length < PIN_CAP) reorderPinned.mutate([...ids, id])
  }

  const close = useCallback(() => {
    setAdding(false)
    setNewName('')
    setRenaming(null)
    setConfirmDelete(null)
    setCtx(null)
    void commands.hideOverlay('bookmarks').catch(() => undefined)
    void emit(BOOKMARKS_CLOSED_EVENT).catch(() => undefined)
  }, [])

  // Take focus when first shown (the chrome shows/hides the persistent overlay).
  useEffect(() => {
    void commands.focusSelf().catch(() => undefined)
  }, [])

  // Esc closes.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      if (ctxRef.current) setCtx(null)
      else close()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [close])

  const submitNewFolder = () => {
    const name = newName.trim()
    if (name) createFolder.mutate(name)
    setNewName('')
    setAdding(false)
  }

  const openBookmark = (b: Bookmark) => {
    if (draggedRef.current) {
      draggedRef.current = false
      return
    }
    if (activeTab && isChromeUrl(activeTab.url)) {
      void commands.navigateTab(activeTab.id, b.url).catch(() => undefined)
    } else if (workspaceId) {
      void commands.createTab(workspaceId, b.url).catch(() => undefined)
    }
    close()
  }

  // Middle-click: open in a background tab and keep the menu up.
  const openBookmarkBackground = (b: Bookmark) => {
    if (workspaceId) {
      void commands.createBackgroundTab(workspaceId, b.url).catch(() => undefined)
    }
  }

  const auxOpen = (e: ReactMouseEvent, b: Bookmark) => {
    if (e.button !== 1) return
    e.preventDefault()
    openBookmarkBackground(b)
  }

  const preemptAutoscroll = (e: ReactMouseEvent) => {
    if (e.button === 1) e.preventDefault()
  }

  const toggleFolder = (id: number) =>
    setOpenFolders((s) => {
      const next = new Set(s)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })

  const ctxFolder = ctx
    ? (allBookmarks.find((b) => b.id === ctx.id)?.folder_id ?? null)
    : null

  return (
    <div className="kb-bmmenu-backdrop" onMouseDown={close} role="presentation">
      <div
        className={cx('kb-bmmenu', `side-${side}`, {
          'glass-menus': glassMenus,
          'is-top-tabs': layout === 'top_tabs',
        })}
        role="dialog"
        aria-label="Bookmarks"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="kb-bmmenu-head">
          <span className="kb-bmmenu-title">Bookmarks</span>
          <button
            type="button"
            className="kb-bmmenu-add"
            aria-label="New folder"
            title="New folder"
            onClick={() => setAdding((v) => !v)}
          >
            <PlusIcon />
          </button>
        </div>

        {adding && (
          <input
            className="kb-bmmenu-input"
            autoFocus
            placeholder="Folder name"
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
        )}

        <div className="kb-bmmenu-scroll">
          <div
            className="kb-bmmenu-bubbles"
            onDragOver={onBubbleAreaDragOver}
            onDrop={onBubbleAreaDrop}
          >
            {pinned.map((b, i) => (
              <div
                key={b.id}
                className={cx('kb-bmmenu-bubble-wrap', { 'is-drop': dropBubble === i })}
                onDragEnter={(e) => e.preventDefault()}
                onDragOver={(e) => onBubbleDragOver(e, i)}
                onDrop={(e) => onBubbleDrop(e, i)}
              >
                <button
                  type="button"
                  className="kb-bmmenu-bubble"
                  title={b.title || b.url}
                  draggable
                  onDragStart={(e) => onFaviconDragStart(e, b.id)}
                  onDragEnd={onFaviconDragEnd}
                  onContextMenu={(e) => openCtx(e, b.id)}
                  onClick={() => openBookmark(b)}
                  onMouseDown={preemptAutoscroll}
                  onAuxClick={(e) => auxOpen(e, b)}
                >
                  <BookmarkFavicon bm={b} />
                </button>
                <button
                  type="button"
                  className="kb-bmmenu-bubble-x"
                  aria-label="Unpin bookmark"
                  title="Unpin"
                  onClick={() => setPinned.mutate({ id: b.id, pinned: false })}
                >
                  <XIcon />
                </button>
              </div>
            ))}
            {pinned.length === 0 && (
              <div className="kb-bmmenu-bubbles-hint">Drag a favicon here to pin it</div>
            )}
          </div>

          {folderList.map((f) => {
            const items = allBookmarks
              .filter((b) => b.folder_id === f.id)
              .sort((a, b) => a.position - b.position)
            const isOpen = openFolders.has(f.id)
            return (
              <div
                key={f.id}
                className={cx('kb-bmmenu-folder', {
                  'is-drop': dropFolder === f.id,
                  'is-reorder': reorderOver === f.id,
                })}
                onDragEnter={(e) => e.preventDefault()}
                onDragOver={(e) => onFolderDragOver(e, f.id)}
                onDragLeave={() => onFolderDragLeave(f.id)}
                onDrop={(e) => onFolderDrop(e, f.id)}
              >
                <div className="kb-bmmenu-folder-head">
                  <span
                    className="kb-bmmenu-folder-grip"
                    role="button"
                    aria-label="Drag to reorder folder"
                    title="Drag to reorder"
                    draggable={renaming !== f.id}
                    onDragStart={(e) => onFolderDragStart(e, f.id)}
                    onDragEnd={onFolderDragEnd}
                  >
                    <GripIcon />
                  </span>
                  <button
                    type="button"
                    className="kb-bmmenu-folder-toggle"
                    onClick={() => toggleFolder(f.id)}
                  >
                    <Caret open={isOpen} />
                    {renaming === f.id ? (
                      <input
                        className="kb-bmmenu-input kb-bmmenu-rename"
                        autoFocus
                        value={renameValue}
                        onClick={(e) => e.stopPropagation()}
                        onChange={(e) => setRenameValue(e.target.value)}
                        onKeyDown={(e) => {
                          e.stopPropagation()
                          if (e.key === 'Enter') {
                            const name = renameValue.trim()
                            if (name) renameFolder.mutate({ id: f.id, name })
                            setRenaming(null)
                          } else if (e.key === 'Escape') setRenaming(null)
                        }}
                        onBlur={() => {
                          const name = renameValue.trim()
                          if (name && name !== f.name) renameFolder.mutate({ id: f.id, name })
                          setRenaming(null)
                        }}
                      />
                    ) : (
                      <span
                        className="kb-bmmenu-folder-name"
                        onDoubleClick={(e) => {
                          e.stopPropagation()
                          setRenaming(f.id)
                          setRenameValue(f.name)
                        }}
                      >
                        {f.name}
                      </span>
                    )}
                    <span className="kb-bmmenu-folder-count">{items.length}</span>
                  </button>
                  {confirmDelete === f.id ? (
                    <span className="kb-bmmenu-confirm">
                      <button
                        type="button"
                        className="kb-bmmenu-confirm-yes"
                        onClick={() => {
                          deleteFolder.mutate(f.id)
                          setConfirmDelete(null)
                        }}
                      >
                        Delete
                      </button>
                      <button
                        type="button"
                        className="kb-bmmenu-confirm-no"
                        onClick={() => setConfirmDelete(null)}
                      >
                        Cancel
                      </button>
                    </span>
                  ) : (
                    <button
                      type="button"
                      className="kb-bmmenu-folder-del"
                      aria-label="Delete folder"
                      title="Delete folder (bookmarks move to unfiled)"
                      onClick={() => setConfirmDelete(f.id)}
                    >
                      <XIcon />
                    </button>
                  )}
                </div>
                {isOpen && (
                  <div className="kb-bmmenu-folder-items">
                    {items.length === 0 ? (
                      <div className="kb-bmmenu-empty">Empty</div>
                    ) : (
                      items.map((b) => (
                        <div
                          key={b.id}
                          className={cx('kb-bmmenu-row', {
                            'is-drop-before': dropRow?.id === b.id && !dropRow.after,
                            'is-drop-after': dropRow?.id === b.id && dropRow.after,
                          })}
                          onDragOver={(e) => onRowDragOver(e, f.id, b.id)}
                          onDragLeave={() => onRowDragLeave(b.id)}
                          onDrop={(e) => onRowDrop(e, f.id, b.id)}
                        >
                          <button
                            type="button"
                            className="kb-bmmenu-row-open"
                            title={b.title || b.url}
                            draggable
                            onDragStart={(e) => onFaviconDragStart(e, b.id)}
                            onDragEnd={onFaviconDragEnd}
                            onContextMenu={(e) => openCtx(e, b.id)}
                            onClick={() => openBookmark(b)}
                            onMouseDown={preemptAutoscroll}
                            onAuxClick={(e) => auxOpen(e, b)}
                          >
                            <span className="kb-bmmenu-row-fav">
                              <BookmarkFavicon bm={b} />
                            </span>
                            <span className="kb-bmmenu-row-title">{b.title || b.url}</span>
                          </button>
                          <button
                            type="button"
                            className="kb-bmmenu-row-x"
                            aria-label="Remove bookmark"
                            onClick={() => deleteBookmark.mutate(b.id)}
                          >
                            <XIcon />
                          </button>
                        </div>
                      ))
                    )}
                  </div>
                )}
              </div>
            )
          })}

          {(restUnfiled.length > 0 || folderList.length > 0) && (
            <div
              className={cx('kb-bmmenu-rest', { 'is-drop': dropUnfiled })}
              onDragOver={onUnfiledDragOver}
              onDragLeave={onUnfiledDragLeave}
              onDrop={onUnfiledDrop}
            >
              {restUnfiled.map((b) => (
                <div key={b.id} className="kb-bmmenu-row">
                  <button
                    type="button"
                    className="kb-bmmenu-row-open"
                    title={b.title || b.url}
                    draggable
                    onDragStart={(e) => onFaviconDragStart(e, b.id)}
                    onDragEnd={onFaviconDragEnd}
                    onContextMenu={(e) => openCtx(e, b.id)}
                    onClick={() => openBookmark(b)}
                    onMouseDown={preemptAutoscroll}
                    onAuxClick={(e) => auxOpen(e, b)}
                  >
                    <span className="kb-bmmenu-row-fav">
                      <BookmarkFavicon bm={b} />
                    </span>
                    <span className="kb-bmmenu-row-title">{b.title || b.url}</span>
                  </button>
                  <button
                    type="button"
                    className="kb-bmmenu-row-x"
                    aria-label="Remove bookmark"
                    onClick={() => deleteBookmark.mutate(b.id)}
                  >
                    <XIcon />
                  </button>
                </div>
              ))}
              {restUnfiled.length === 0 && (
                <div className="kb-bmmenu-rest-hint">Drag here to remove from a folder</div>
              )}
            </div>
          )}

          {allBookmarks.length === 0 && folderList.length === 0 && (
            <div className="kb-bmmenu-empty kb-bmmenu-empty-all">
              No bookmarks yet. Use the star in the address bar to add this page.
            </div>
          )}
        </div>
      </div>

      {ctx && (
        <>
          <div
            className="kb-bmctx-catch"
            onMouseDown={(e) => {
              e.stopPropagation()
              setCtx(null)
            }}
          />
          <div
            className={cx('kb-bmctx', { 'glass-menus': glassMenus })}
            style={{ left: Math.min(ctx.x, window.innerWidth - 210), top: ctx.y }}
            role="menu"
            onKeyDown={(e) => moveFocusByArrow(e, e.currentTarget, 'button', 'vertical')}
            onMouseDown={(e) => e.stopPropagation()}
          >
            {isPinned(ctx.id) ? (
              <button
                type="button"
                role="menuitem"
                className="kb-bmctx-row"
                onClick={() => {
                  setPinned.mutate({ id: ctx.id, pinned: false })
                  setCtx(null)
                }}
              >
                Unpin
              </button>
            ) : (
              <button
                type="button"
                role="menuitem"
                className="kb-bmctx-row"
                onClick={() => {
                  setPinned.mutate({ id: ctx.id, pinned: true })
                  setCtx(null)
                }}
              >
                Pin to top
              </button>
            )}
            <div className="kb-bmctx-sep" role="separator" />
            <div className="kb-bmctx-label">Move to</div>
            <button
              type="button"
              className={cx('kb-bmctx-row', { 'is-sel': ctxFolder == null })}
              role="menuitemradio"
              aria-checked={ctxFolder == null}
              onClick={() => assignCtx(null)}
            >
              Unfiled
            </button>
            {folderList.map((f) => (
              <button
                key={f.id}
                type="button"
                className={cx('kb-bmctx-row', { 'is-sel': ctxFolder === f.id })}
                role="menuitemradio"
                aria-checked={ctxFolder === f.id}
                onClick={() => assignCtx(f.id)}
              >
                {f.name}
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  )
}

function Caret({ open }: { open: boolean }) {
  return (
    <svg
      className={cx('kb-bmmenu-caret', { open })}
      width={12}
      height={12}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <polyline points="6 4 10 8 6 12" />
    </svg>
  )
}

function GripIcon() {
  return (
    <svg width={10} height={14} viewBox="0 0 10 14" fill="currentColor" aria-hidden>
      <circle cx="2.5" cy="3" r="1.2" />
      <circle cx="7.5" cy="3" r="1.2" />
      <circle cx="2.5" cy="7" r="1.2" />
      <circle cx="7.5" cy="7" r="1.2" />
      <circle cx="2.5" cy="11" r="1.2" />
      <circle cx="7.5" cy="11" r="1.2" />
    </svg>
  )
}
