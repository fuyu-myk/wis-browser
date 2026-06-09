import { useCallback, useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { AccentColor, TabFolder, TabFolderId, TabId, WorkspaceId, Tab } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { FolderPlusIcon, PlusIcon, XIcon } from '@/lib/icons'
import { tabDisplayName } from '@/lib/url'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'
import {
  useTabSplit,
  tabDropTarget,
  reorderedAmong,
  type TabReorder,
  type DropTarget,
} from '@/hooks/useTabSplit'

import { TabFavicon } from '@/components/TabFavicon'
import { TabFolderHeader } from '@/components/TabFolderHeader'
import { useDiscardedTabs } from '@/components/DiscardedTabsContext'

interface TabListProps {
  workspaceId: WorkspaceId
  tabs: Tab[]
  activeTabId: TabId | null
  /** Pinned tab IDs; rendered in the quick-access grid, not this list. */
  pinnedTabIds: TabId[]
  /** Live-tab folders for this workspace. */
  folders: TabFolder[]
  /** Folder currently targeted for new tabs (highlighted), or null. */
  selectedFolderId: TabFolderId | null
  /** Notified when the right-click split menu opens/closes (expand sidebar). */
  onSplitMenuOpenChange?: (open: boolean) => void
  /** Notified while any folder is being renamed/edited (keep sidebar open). */
  onFolderEditingChange?: (editing: boolean) => void
}

const ACCENTS: AccentColor[] = [
  'primary',
  'mint',
  'rose',
  'sun',
  'lila',
  'gray',
  'ink',
  'azur',
]

/** A non-reorder drop target signalling "assign to folder", not "insert here". */
const FOLDER_DROP: DropTarget = { index: -1, anchorIndex: -1, side: 'before' }

/** Move `id` to `insertion` within `order`; null when nothing changes. */
function moveInList<T>(order: T[], id: T, insertion: number): T[] | null {
  const from = order.indexOf(id)
  if (from < 0) return null
  const without = order.filter((x) => x !== id)
  const at = insertion > from ? insertion - 1 : insertion
  without.splice(at, 0, id)
  if (without.length === order.length && without.every((x, i) => x === order[i])) return null
  return without
}

/**
 * Vertical tab list shown inside the sidebar: a `+ New tab` row, the loose
 * (un-foldered) tabs as a flat list, then live-tab folders rendered as an
 * IDE-style tree. A tab can be dragged onto a folder header to file it, or out
 * onto the loose area to unfile it; loose tabs reorder among themselves.
 */
export function TabList({
  workspaceId,
  tabs,
  activeTabId,
  pinnedTabIds,
  folders,
  selectedFolderId,
  onSplitMenuOpenChange,
  onFolderEditingChange,
}: TabListProps) {
  const qc = useQueryClient()
  const invalidate = () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY })
  const setActive = useMutation({
    mutationFn: (id: TabId) => commands.setActiveTab(id),
    onSuccess: invalidate,
  })
  const close = useMutation({
    mutationFn: (id: TabId) => commands.closeTab(id),
    onSuccess: invalidate,
  })
  const createTab = useMutation({
    // The explicit "+ New tab" button always makes a loose, top-level tab; only
    // Cmd+T and link/bookmark opens honour the selected folder.
    mutationFn: () => commands.createLooseTab(workspaceId, 'about:blank'),
    onSuccess: invalidate,
  })
  const reorder = useMutation({
    mutationFn: (order: TabId[]) => commands.reorderTabs(workspaceId, order),
    onSuccess: invalidate,
  })
  const assign = useMutation({
    mutationFn: (args: { tabId: TabId; folderId: TabFolderId | null }) =>
      commands.assignTabToFolder(args.tabId, args.folderId),
    onSuccess: invalidate,
  })
  const createFolder = useMutation({
    mutationFn: () =>
      commands.createTabFolder(
        workspaceId,
        'New folder',
        ACCENTS[folders.length % ACCENTS.length],
      ),
    onSuccess: invalidate,
  })
  const reorderFolders = useMutation({
    mutationFn: (order: TabFolderId[]) => commands.reorderTabFolders(workspaceId, order),
    onSuccess: invalidate,
  })

  const { startTabDrag, draggedRef, openSplitMenu } = useTabSplit({
    workspaceId,
    activeTabId,
    onMenuOpenChange: onSplitMenuOpenChange,
  })

  const discardedTabs = useDiscardedTabs()
  const looseRef = useRef<HTMLDivElement>(null)
  const rootRef = useRef<HTMLDivElement>(null)
  const [dropTarget, setDropTarget] = useState<DropTarget | null>(null)
  // `undefined` while not over a folder zone; a folder id, or `null` for the
  // "loose" zone. Read at drop time (refs don't go stale like state can).
  const folderDropRef = useRef<TabFolderId | null | undefined>(undefined)
  const [folderDropHint, setFolderDropHint] = useState<string | null>(null)
  // Which list a tab reorder is happening within: 'loose' or a folder id.
  const dragScopeRef = useRef<string | null>(null)
  // Folder drag-to-reorder (separate from tab dragging).
  const folderDraggedRef = useRef(false)
  const [folderDropIndex, setFolderDropIndex] = useState<number | null>(null)

  const [editingFolders, setEditingFolders] = useState<Set<TabFolderId>>(new Set())
  const reportEditing = useCallback((folderId: TabFolderId, editing: boolean) => {
    setEditingFolders((prev) => {
      if (editing === prev.has(folderId)) return prev
      const next = new Set(prev)
      if (editing) next.add(folderId)
      else next.delete(folderId)
      return next
    })
  }, [])
  useEffect(() => {
    onFolderEditingChange?.(editingFolders.size > 0)
  }, [editingFolders, onFolderEditingChange])

  const pinnedSet = new Set(pinnedTabIds)
  const looseTabs = tabs.filter((t) => !pinnedSet.has(t.id) && t.folder_id == null)
  const looseIds = new Set(looseTabs.map((t) => t.id))
  const sortedFolders = [...folders].sort((a, b) => a.position - b.position)
  const membersOf = (folderId: TabFolderId) =>
    tabs.filter((t) => !pinnedSet.has(t.id) && t.folder_id === folderId)

  const folderZoneAt = (x: number, y: number): TabFolderId | undefined => {
    const root = rootRef.current
    if (!root) return undefined
    for (const el of root.querySelectorAll<HTMLElement>('[data-folder-zone]')) {
      const r = el.getBoundingClientRect()
      if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) {
        return el.dataset.folderZone as TabFolderId
      }
    }
    return undefined
  }

  const folderBodyEl = (folderId: TabFolderId): HTMLElement | null =>
    rootRef.current?.querySelector<HTMLElement>(`[data-folder-body="${folderId}"]`) ?? null

  const withinRoot = (x: number, y: number): boolean => {
    const r = rootRef.current?.getBoundingClientRect()
    return !!r && x >= r.left && x <= r.right && y >= r.top && y <= r.bottom
  }

  const makeReorder = (tab: Tab): TabReorder => ({
    targetAtPoint: (x, y) => {
      const zone = folderZoneAt(x, y)
      if (tab.folder_id == null) {
        // Loose tab: drop onto a folder to file it, else reorder among loose.
        if (zone !== undefined) {
          folderDropRef.current = zone
          setFolderDropHint(zone)
          dragScopeRef.current = null
          return FOLDER_DROP
        }
        folderDropRef.current = undefined
        setFolderDropHint(null)
        dragScopeRef.current = 'loose'
        return looseRef.current ? tabDropTarget(looseRef.current, 'y', x, y) : null
      }
      // Member tab: reorder within its own folder, move to another, or out.
      if (zone === tab.folder_id) {
        const body = folderBodyEl(tab.folder_id)
        const t = body ? tabDropTarget(body, 'y', x, y) : null
        folderDropRef.current = undefined
        setFolderDropHint(null)
        dragScopeRef.current = t ? tab.folder_id : null
        return t ?? FOLDER_DROP
      }
      if (zone !== undefined) {
        folderDropRef.current = zone
        setFolderDropHint(zone)
        dragScopeRef.current = null
        return FOLDER_DROP
      }
      if (withinRoot(x, y)) {
        // Empty / loose area: take the tab out of its folder.
        folderDropRef.current = null
        setFolderDropHint('loose')
        dragScopeRef.current = null
        return FOLDER_DROP
      }
      folderDropRef.current = undefined
      setFolderDropHint(null)
      dragScopeRef.current = null
      return null
    },
    onTarget: setDropTarget,
    onDrop: (idx) => {
      const drop = folderDropRef.current
      const scope = dragScopeRef.current
      folderDropRef.current = undefined
      setFolderDropHint(null)
      dragScopeRef.current = null
      if (drop !== undefined) {
        if (drop !== (tab.folder_id ?? null)) {
          assign.mutate({ tabId: tab.id, folderId: drop })
        }
        return
      }
      const fullOrder = tabs.map((t) => t.id)
      if (scope === 'loose' && tab.folder_id == null) {
        const next = reorderedAmong(fullOrder, looseIds, tab.id, idx)
        if (next) reorder.mutate(next)
      } else if (scope && tab.folder_id != null && scope === tab.folder_id) {
        const memberIds = new Set(membersOf(tab.folder_id).map((m) => m.id))
        const next = reorderedAmong(fullOrder, memberIds, tab.id, idx)
        if (next) reorder.mutate(next)
      }
    },
  })

  const startFolderDrag = (e: ReactPointerEvent, folderId: TabFolderId) => {
    if (e.button !== 0) return
    if ((e.target as HTMLElement).closest('button, input')) return
    const startX = e.clientX
    const startY = e.clientY
    const sourceEl = e.currentTarget as HTMLElement
    const rect = sourceEl.getBoundingClientRect()
    const grabX = startX - rect.left
    const grabY = startY - rect.top
    const accent = sortedFolders.find((f) => f.id === folderId)?.accent
    const order = sortedFolders.map((f) => f.id)
    folderDraggedRef.current = false
    let begun = false
    let ghost: HTMLElement | null = null
    let lastIndex: number | null = null

    const insertionAt = (y: number): number => {
      const root = rootRef.current
      if (!root) return order.length
      const heads = Array.from(root.querySelectorAll<HTMLElement>('[data-folder-id]'))
      for (let i = 0; i < heads.length; i++) {
        const r = heads[i].getBoundingClientRect()
        if (y < r.top + r.height / 2) return i
      }
      return heads.length
    }

    const move = (ev: PointerEvent) => {
      if (!begun) {
        if (Math.hypot(ev.clientX - startX, ev.clientY - startY) <= 6) return
        begun = true
        folderDraggedRef.current = true
        document.body.classList.add('is-tab-dragging')
        window.getSelection()?.removeAllRanges()
        ghost = sourceEl.cloneNode(true) as HTMLElement
        ghost.classList.add('kb-drag-ghost')
        if (accent) ghost.classList.add(`accent-${accent}`)
        ghost.style.width = `${rect.width}px`
        document.body.appendChild(ghost)
      }
      if (ghost) {
        ghost.style.left = `${ev.clientX - grabX}px`
        ghost.style.top = `${ev.clientY - grabY}px`
      }
      lastIndex = insertionAt(ev.clientY)
      setFolderDropIndex(lastIndex)
    }
    const up = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      ghost?.remove()
      if (begun) document.body.classList.remove('is-tab-dragging')
      setFolderDropIndex(null)
      if (begun && lastIndex != null) {
        const next = moveInList(order, folderId, lastIndex)
        if (next) reorderFolders.mutate(next)
      }
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  const renderTab = (tab: Tab, opts: { scope?: string; index?: number; inFolder?: boolean } = {}) => {
    const showDrop =
      opts.scope !== undefined &&
      dragScopeRef.current === opts.scope &&
      dropTarget?.anchorIndex === opts.index
    return (
      <div
        key={tab.id}
        data-tab-id={tab.id}
        className={cx('kb-tab', {
          active: tab.id === activeTabId,
          'in-folder': opts.inFolder,
          asleep: discardedTabs.has(tab.id) && tab.id !== activeTabId,
          'drop-before': showDrop && dropTarget?.side === 'before',
          'drop-after': showDrop && dropTarget?.side === 'after',
        })}
        role="button"
        tabIndex={0}
        onPointerDown={(e) => startTabDrag(e, tab.id, makeReorder(tab))}
        onClick={() => {
          if (draggedRef.current) {
            draggedRef.current = false
            return
          }
          setActive.mutate(tab.id)
        }}
        onContextMenu={(e) => openSplitMenu(e, tab.id)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault()
            setActive.mutate(tab.id)
          }
        }}
      >
        <TabFavicon tab={tab} />
        <span className="ttl">{tabDisplayName(tab)}</span>
        <button
          type="button"
          className="close"
          aria-label="Close tab"
          onClick={(e) => {
            e.stopPropagation()
            close.mutate(tab.id)
          }}
        >
          <XIcon />
        </button>
      </div>
    )
  }

  return (
    <div className="kb-tabs" ref={rootRef}>
      <div className="group-label">tabs</div>
      <button
        type="button"
        className="kb-newtab-row"
        onClick={() => createTab.mutate()}
        disabled={!workspaceId || createTab.isPending}
      >
        <PlusIcon />
        <span>New tab</span>
      </button>

      {sortedFolders.map((folder, idx) => {
        const members = membersOf(folder.id)
        return (
          <div
            key={folder.id}
            data-folder-zone={folder.id}
            className={cx('kb-tabfolder', `accent-${folder.accent}`, {
              'drop-before': folderDropIndex === idx,
              'drop-after':
                folderDropIndex === sortedFolders.length && idx === sortedFolders.length - 1,
            })}
          >
            <TabFolderHeader
              workspaceId={workspaceId}
              folder={folder}
              count={members.length}
              isDropTarget={folderDropHint === folder.id}
              selected={selectedFolderId === folder.id}
              containsActive={activeTabId != null && members.some((m) => m.id === activeTabId)}
              dragRef={folderDraggedRef}
              onHeaderPointerDown={(e) => startFolderDrag(e, folder.id)}
              onEditingChange={(e) => reportEditing(folder.id, e)}
            />
            {!folder.collapsed && (
              <div className="kb-tabfolder-body" data-folder-body={folder.id}>
                {members.map((tab, mi) =>
                  renderTab(tab, { scope: folder.id, index: mi, inFolder: true }),
                )}
              </div>
            )}
          </div>
        )
      })}

      <button
        type="button"
        className="kb-newfolder-row"
        onClick={() => createFolder.mutate()}
        disabled={!workspaceId || createFolder.isPending}
      >
        <FolderPlusIcon />
        <span>New folder</span>
      </button>

      <div
        className={cx('kb-loose', { 'is-drop': folderDropHint === 'loose' })}
        ref={looseRef}
      >
        {looseTabs.map((tab, i) => renderTab(tab, { scope: 'loose', index: i }))}
      </div>
    </div>
  )
}

export type { Tab }
