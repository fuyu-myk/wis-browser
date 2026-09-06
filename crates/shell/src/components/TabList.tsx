import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'


import { commands } from '@/bindings/commands'
import { closeTabAnimated, onTabClosing, TAB_EXIT_MS } from '@/lib/tabClose'
import { closeReturnTarget, noteTabClosed } from '@/lib/tabMru'
import type {
  BrowserState,
  TabFolder,
  TabFolderId,
  TabId,
  WorkspaceId,
  Tab,
} from '@/bindings/types'
import { FOLDER_ACCENTS } from '@/lib/accents'
import { cx } from '@/lib/cx'
import { FolderPlusIcon, PlusIcon, XIcon } from '@/lib/icons'
import { resolveNewTabUrl, tabDisplayName } from '@/lib/url'
import {
  partOffset,
  patchFolderOrder,
  patchTabFolder,
  patchTabOrder,
  runPendingGlide,
  type PendingGlide,
} from '@/lib/tabMotion'
import { useWorkspaceSlide } from '@/hooks/useWorkspaceSlide'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'
import {
  useTabSplit,
  tabDropTarget,
  insertedAmong,
  reorderedAmong,
  type TabReorder,
  type DropTarget,
} from '@/hooks/useTabSplit'

import { TabAudioButton } from '@/components/TabAudioButton'
import { TabFavicon } from '@/components/TabFavicon'
import { TabFolderHeader } from '@/components/TabFolderHeader'
import { useDiscardedTabs } from '@/components/DiscardedTabsContext'
import { useTabAudio } from '@/hooks/useTabAudio'

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
  /** Workspace ids in display order, for the directional switch slide. */
  workspaceOrder?: WorkspaceId[]
}

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
  workspaceOrder,
}: TabListProps) {
  const qc = useQueryClient()
  const invalidate = () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY })

  // Spawn/close choreography: ids appearing after first render animate in;
  // a closing tab plays its exit before the close command removes it.
  const knownIdsRef = useRef<Set<TabId> | null>(null)
  const currentIds = new Set(tabs.map((t) => t.id))
  const newIds =
    knownIdsRef.current === null
      ? new Set<TabId>()
      : new Set(tabs.map((t) => t.id).filter((id) => !knownIdsRef.current?.has(id)))
  useEffect(() => {
    knownIdsRef.current = currentIds
  })
  const [closingIds, setClosingIds] = useState<ReadonlySet<TabId>>(new Set())
  useEffect(
    () =>
      onTabClosing((id) => {
        setClosingIds((prev) => new Set(prev).add(id))
        window.setTimeout(() => {
          setClosingIds((prev) => {
            const next = new Set(prev)
            next.delete(id)
            return next
          })
        }, TAB_EXIT_MS + 400)
      }),
    [],
  )
  const tabAudio = useTabAudio()
  const setActive = useMutation({
    mutationFn: (id: TabId) => commands.setActiveTab(id),
    onSuccess: invalidate,
  })
  const close = useMutation({
    mutationFn: (id: TabId) => {
      const returnTo = closeReturnTarget(id)
      noteTabClosed(id)
      return commands.closeTab(id, returnTo)
    },
    onSuccess: invalidate,
  })
  const createTab = useMutation({
    // The explicit "+ New tab" button always makes a loose, top-level tab; only
    // Cmd+T and link/bookmark opens honour the selected folder. Read settings
    // from the cache at click time so the button honours the new-tab target
    // (homepage vs launchpad) without subscribing the strip to re-renders.
    mutationFn: () => {
      const settings = qc.getQueryData<BrowserState>([...BROWSER_STATE_KEY])?.settings
      return commands.createLooseTab(workspaceId, resolveNewTabUrl(settings))
    },
    onSuccess: invalidate,
  })
  const reorder = useMutation({
    mutationFn: (order: TabId[]) => commands.reorderTabs(workspaceId, order),
    // Optimistic: a drop re-renders in the new order on the same frame.
    onMutate: (order) => patchTabOrder(qc, workspaceId, order),
    onSuccess: invalidate,
  })
  const assign = useMutation({
    mutationFn: (args: { tabId: TabId; folderId: TabFolderId | null }) =>
      commands.assignTabToFolder(args.tabId, args.folderId),
    // Optimistic: filing renders on the drop frame.
    onMutate: (args) => patchTabFolder(qc, args.tabId, args.folderId),
    onSuccess: invalidate,
  })
  const createFolder = useMutation({
    mutationFn: () =>
      commands.createTabFolder(
        workspaceId,
        'New folder',
        FOLDER_ACCENTS[folders.length % FOLDER_ACCENTS.length],
      ),
    onSuccess: invalidate,
  })
  const reorderFolders = useMutation({
    mutationFn: (order: TabFolderId[]) => commands.reorderTabFolders(workspaceId, order),
    onMutate: (order) => patchFolderOrder(qc, workspaceId, order),
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
  // Live parting.
  const [dragPart, setDragPart] = useState<{ id: TabId; size: number } | null>(null)
  const [folderDragPart, setFolderDragPart] = useState<{
    id: TabFolderId
    size: number
  } | null>(null)
  const pendingGlideRef = useRef<PendingGlide | null>(null)
  // Land the dropped element once the optimistic reorder has rendered.
  useLayoutEffect(() => {
    runPendingGlide(rootRef.current, pendingGlideRef)
  })

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

  // Optimistic close: the backend removes the tab immediately, while the
  // strip keeps a ghost row in place until the exit animation finishes.
  const prevTabsRef = useRef<Tab[]>(tabs)
  const displayTabs = (() => {
    if (closingIds.size === 0) return tabs
    const have = new Set(tabs.map((t) => t.id))
    const prev = prevTabsRef.current
    const ghosts = prev.filter((t) => closingIds.has(t.id) && !have.has(t.id))
    if (ghosts.length === 0) return tabs
    const merged = [...tabs]
    for (const g of ghosts) {
      merged.splice(
        Math.min(
          prev.findIndex((t) => t.id === g.id),
          merged.length,
        ),
        0,
        g,
      )
    }
    return merged
  })()
  useEffect(() => {
    prevTabsRef.current = displayTabs
  })

  const pinnedSet = new Set(pinnedTabIds)
  const looseTabs = displayTabs.filter((t) => !pinnedSet.has(t.id) && t.folder_id == null)
  const looseIds = new Set(looseTabs.map((t) => t.id))
  const sortedFolders = [...folders].sort((a, b) => a.position - b.position)
  const membersOf = (folderId: TabFolderId) =>
    displayTabs.filter((t) => !pinnedSet.has(t.id) && t.folder_id === folderId)

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

  // Workspace switches slide the list in from the travel direction.
  useWorkspaceSlide('sidebar-tabs', workspaceId, workspaceOrder, rootRef)

  // Sliding active-tab bar.
  const [barTop, setBarTop] = useState<number | null>(null)
  const measureBar = useCallback(() => {
    const root = rootRef.current
    const el = activeTabId
      ? root?.querySelector<HTMLElement>(`[data-tab-id="${activeTabId}"]`)
      : null
    const inner = el?.closest<HTMLElement>('.kb-foldwrap-inner')
    if (
      !root ||
      !el ||
      el.closest('.kb-foldwrap:not(.is-open)') ||
      (inner !== null && inner !== undefined && inner.getBoundingClientRect().height < 4)
    ) {
      setBarTop(null)
      return
    }
    const rootRect = root.getBoundingClientRect()
    const r = el.getBoundingClientRect()
    // The bar is absolutely positioned inside the scrolling list, so its
    // `top` is in content coordinates: add the scroll offset.
    const next = r.top - rootRect.top + root.scrollTop + r.height / 2 - 9
    setBarTop((prev) => (prev !== null && Math.abs(prev - next) < 0.5 ? prev : next))
  }, [activeTabId])
  useLayoutEffect(() => {
    measureBar()
  })
  useEffect(() => {
    const root = rootRef.current
    if (!root) return
    const onEnd = (e: TransitionEvent) => {
      if (e.propertyName === 'grid-template-rows') measureBar()
    }
    root.addEventListener('transitionend', onEnd)
    return () => root.removeEventListener('transitionend', onEnd)
  }, [measureBar])

  const folderIsOpen = (id: TabFolderId): boolean =>
    sortedFolders.find((f) => f.id === id)?.collapsed === false

  const makeReorder = (tab: Tab): TabReorder => ({
    targetAtPoint: (x, y) => {
      const zone = folderZoneAt(x, y)
      // Foreign open folder.
      if (zone !== undefined && zone !== tab.folder_id && folderIsOpen(zone)) {
        const body = folderBodyEl(zone)
        const t = body ? tabDropTarget(body, 'y', x, y) : null
        if (t) {
          folderDropRef.current = undefined
          setFolderDropHint(null)
          dragScopeRef.current = zone
          return t
        }
      }
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
        // Loose area.
        const t = looseRef.current ? tabDropTarget(looseRef.current, 'y', x, y) : null
        if (t) {
          folderDropRef.current = undefined
          setFolderDropHint(null)
          dragScopeRef.current = 'loose'
          return t
        }
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
    onDragChange: (dragging) => {
      if (!dragging) {
        setDragPart(null)
        return
      }
      const el = rootRef.current?.querySelector<HTMLElement>(`[data-tab-id="${tab.id}"]`)
      setDragPart({ id: tab.id, size: (el?.getBoundingClientRect().height ?? 29) + 1 })
    },
    onDrop: (idx, ghostRect) => {
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
      const glide = () => {
        if (ghostRect) {
          pendingGlideRef.current = { selector: `[data-tab-id="${tab.id}"]`, rect: ghostRect }
        }
      }
      const fullOrder = tabs.map((t) => t.id)
      if (scope === 'loose' && tab.folder_id == null) {
        const next = reorderedAmong(fullOrder, looseIds, tab.id, idx)
        if (next) {
          glide()
          reorder.mutate(next)
        }
      } else if (scope === 'loose' && tab.folder_id != null) {
        // Positional unfile into the loose list.
        const next = insertedAmong(fullOrder, looseIds, tab.id, idx)
        glide()
        assign.mutate({ tabId: tab.id, folderId: null })
        if (next) reorder.mutate(next)
      } else if (scope && tab.folder_id != null && scope === tab.folder_id) {
        const memberIds = new Set(membersOf(tab.folder_id).map((m) => m.id))
        const next = reorderedAmong(fullOrder, memberIds, tab.id, idx)
        if (next) {
          glide()
          reorder.mutate(next)
        }
      } else if (scope && scope !== 'loose') {
        // File into an open folder at the targeted slot.
        const folderId = scope as TabFolderId
        const memberIds = new Set(membersOf(folderId).map((m) => m.id))
        const next = insertedAmong(fullOrder, memberIds, tab.id, idx)
        glide()
        assign.mutate({ tabId: tab.id, folderId })
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
        const zone = rootRef.current?.querySelector<HTMLElement>(
          `[data-folder-zone="${folderId}"]`,
        )
        setFolderDragPart({
          id: folderId,
          size: (zone?.getBoundingClientRect().height ?? rect.height) + 2,
        })
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
      const ghostRect = ghost?.getBoundingClientRect()
      ghost?.remove()
      if (begun) document.body.classList.remove('is-tab-dragging')
      setFolderDropIndex(null)
      setFolderDragPart(null)
      if (begun && lastIndex != null) {
        const next = moveInList(order, folderId, lastIndex)
        if (next) {
          if (ghostRect) {
            pendingGlideRef.current = {
              selector: `[data-folder-id="${folderId}"]`,
              rect: ghostRect,
            }
          }
          reorderFolders.mutate(next)
        }
      }
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  const renderTab = (
    tab: Tab,
    opts: { scope?: string; index?: number; inFolder?: boolean; fromIndex?: number } = {},
  ) => {
    const parting =
      dragPart !== null &&
      dropTarget !== null &&
      dropTarget.index >= 0 &&
      opts.scope !== undefined &&
      dragScopeRef.current === opts.scope &&
      opts.fromIndex !== undefined
    const shift = parting ? partOffset(opts.index ?? -1, opts.fromIndex ?? -1, dropTarget.index) : 0
    const isSource = dragPart?.id === tab.id
    // Optimistic resize.
    const lifted = isSource && dragScopeRef.current !== opts.scope
    return (
      <div
        key={tab.id}
        data-tab-id={tab.id}
        className={cx('kb-tab', {
          'is-new': newIds.has(tab.id),
          'is-closing': closingIds.has(tab.id),
          active: tab.id === activeTabId,
          'in-folder': opts.inFolder,
          asleep: discardedTabs.has(tab.id) && tab.id !== activeTabId,
          'is-dragsource': isSource,
          'is-lifted': lifted,
        })}
        style={
          shift !== 0 && dragPart
            ? { transform: `translateY(${shift * dragPart.size}px)` }
            : undefined
        }
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
        onMouseDown={(e) => {
          if (e.button === 1) e.preventDefault()
        }}
        onAuxClick={(e) => {
          if (e.button === 1) {
            e.preventDefault()
            closeTabAnimated(tab.id)
          }
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault()
            setActive.mutate(tab.id)
          }
        }}
      >
        <TabFavicon tab={tab} />
        <TabAudioButton
          id={tab.id}
          audible={tabAudio.audible.has(tab.id)}
          muted={tabAudio.muted.has(tab.id)}
          onToggle={tabAudio.toggleMute}
        />
        <span className="ttl">{tabDisplayName(tab)}</span>
        <button
          type="button"
          className="close"
          aria-label="Close tab"
          onClick={(e) => {
            e.stopPropagation()
            closeTabAnimated(tab.id)
          }}
        >
          <XIcon />
        </button>
      </div>
    )
  }

  const looseFrom = dragPart ? looseTabs.findIndex((t) => t.id === dragPart.id) : -1

  // Optimistic expansion.
  const expandFor = (scopeId: string, list: Tab[]): number =>
    dragPart !== null &&
    dragScopeRef.current === scopeId &&
    dropTarget !== null &&
    dropTarget.index >= 0 &&
    !list.some((t) => t.id === dragPart.id)
      ? dragPart.size
      : 0

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
      <button
        type="button"
        className="kb-newfolder-row"
        onClick={() => createFolder.mutate()}
        disabled={!workspaceId || createFolder.isPending}
      >
        <FolderPlusIcon />
        <span>New folder</span>
      </button>

      {sortedFolders.map((folder, idx) => {
        const members = membersOf(folder.id)
        const folderShift =
          folderDragPart && folderDropIndex !== null
            ? partOffset(
                idx,
                sortedFolders.findIndex((f) => f.id === folderDragPart.id),
                folderDropIndex,
              )
            : 0
        return (
          <div
            key={folder.id}
            data-folder-zone={folder.id}
            className={cx('kb-tabfolder', `accent-${folder.accent}`, {
              'is-dragsource': folderDragPart?.id === folder.id,
            })}
            style={
              folderShift !== 0 && folderDragPart
                ? { transform: `translateY(${folderShift * folderDragPart.size}px)` }
                : undefined
            }
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
            <div className={cx('kb-foldwrap', { 'is-open': !folder.collapsed })}>
              <div className="kb-foldwrap-inner">
                <div
                  className="kb-tabfolder-body"
                  data-folder-body={folder.id}
                  style={{ paddingBottom: expandFor(folder.id, members) || undefined }}
                >
                  {members.map((tab, mi) =>
                    renderTab(tab, {
                      scope: folder.id,
                      index: mi,
                      inFolder: true,
                      fromIndex: dragPart
                        ? members.findIndex((m) => m.id === dragPart.id)
                        : -1,
                    }),
                  )}
                </div>
              </div>
            </div>
          </div>
        )
      })}

      <div
        className={cx('kb-loose', { 'is-drop': folderDropHint === 'loose' })}
        ref={looseRef}
        style={{ paddingBottom: expandFor('loose', looseTabs) || undefined }}
      >
        {looseTabs.map((tab, i) =>
          renderTab(tab, { scope: 'loose', index: i, fromIndex: looseFrom }),
        )}
      </div>
      {barTop !== null && (
        <span
          key={workspaceId}
          className="kb-active-bar"
          style={{ transform: `translateY(${barTop}px)` }}
          aria-hidden
        />
      )}
    </div>
  )
}

export type { Tab }
