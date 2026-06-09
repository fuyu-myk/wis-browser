import {
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
} from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type {
  AccentColor,
  RailSide,
  Tab,
  TabFolder,
  TabFolderId,
  TabId,
  WorkspaceId,
} from '@/bindings/types'
import { cx } from '@/lib/cx'
import { LockIcon, XIcon } from '@/lib/icons'
import { tabDisplayName } from '@/lib/url'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'
import {
  useTabSplit,
  tabDropTarget,
  reorderedAmong,
  type TabReorder,
  type DropTarget,
} from '@/hooks/useTabSplit'

import { useIslandMenu } from '@/hooks/useIslandMenu'
import { TabFavicon } from '@/components/TabFavicon'
import { useDiscardedTabs } from '@/components/DiscardedTabsContext'

interface HorizontalTabsProps {
  workspaceId: WorkspaceId
  tabs: Tab[]
  activeTabId: TabId | null
  /** Pinned tab IDs; pulled out of the strip (they live in the rail). */
  pinnedTabIds: TabId[]
  /** Live-tab folders, rendered as "islands" (collapsed circle / expanded pill). */
  folders: TabFolder[]
  /** The folder new tabs file into, highlighted in the theme colour. */
  selectedFolderId: TabFolderId | null
  /** Rail edge: drives section order, where pinned sits nearest to the rail. */
  railSide: RailSide
  /** The active theme accent; a folder using it needs a contrast active ring. */
  themeAccent: AccentColor
  /** When true, render the URL inside the active tab. */
  integrateUrlBar: boolean
  /** Render as a smaller, centered tab strip. */
  compact?: boolean
}

/* Phase boundary: active tab can't shrink below this (in px). */
const MINIMUM_CURRENT_TAB_SIZE = 260
const COMPACT_MINIMUM_CURRENT_TAB_SIZE = 180
/* Final floor: inactive tabs can't shrink below this (favicon visible). */
const MINIMUM_TAB_SIZE = 44
const COMPACT_MINIMUM_TAB_SIZE = 40
const GAP_PX = 4
/* Fixed widths reserved by islands before sizing the flexible tabs. */
const ISLAND_CIRCLE_W = 30
const ISLAND_NAME_W = 96
const ISLAND_NAME_W_COMPACT = 74
const ISLAND_PILL_PAD = 3
const PIN_CIRCLE_W = 30

/** A non-reorder drop target: "assign to island", not "insert here". */
const ISLAND_DROP: DropTarget = { index: -1, anchorIndex: -1, side: 'before' }

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

interface SizingResult {
  activeSize: number
  inactiveSize: number
  /** True when total content exceeds the strip and scrolling is needed. */
  scrolling: boolean
}

interface FolderGroup {
  folder: TabFolder
  members: Tab[]
}

/**
 * Horizontal tab strip. Loose tabs flex to share the row; live-tab folders
 * render as "islands": a small colored circle when collapsed, or a glass pill
 * (colored name + its tabs) when expanded. Islands take a fixed slice of the
 * row; the rest is shared by the loose and expanded-island tabs.
 */
/**
 * Distribute `budget` (already net of all gaps and the islands' fixed parts)
 * across `n` flexible tabs. The active tab keeps `minActive` when space runs
 * short; below the inactive floor the strip scrolls.
 */
function computeSizing(
  n: number,
  budget: number,
  minActive: number,
  minInactive: number,
): SizingResult {
  if (n <= 0 || budget <= 0) {
    return { activeSize: minActive, inactiveSize: minInactive, scrolling: false }
  }

  const equalSize = budget / n
  if (equalSize >= minActive) {
    return { activeSize: equalSize, inactiveSize: equalSize, scrolling: false }
  }

  if (n === 1) {
    return { activeSize: minActive, inactiveSize: minInactive, scrolling: budget < minActive }
  }
  const calcInactiveSize = (budget - minActive) / (n - 1)
  if (calcInactiveSize >= minInactive) {
    return { activeSize: minActive, inactiveSize: calcInactiveSize, scrolling: false }
  }

  return { activeSize: minActive, inactiveSize: minInactive, scrolling: true }
}

export function HorizontalTabs({
  workspaceId,
  tabs,
  activeTabId,
  pinnedTabIds,
  folders,
  selectedFolderId,
  railSide,
  themeAccent,
  integrateUrlBar,
  compact = false,
}: HorizontalTabsProps) {
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
  const reorder = useMutation({
    mutationFn: (order: TabId[]) => commands.reorderTabs(workspaceId, order),
    onSuccess: invalidate,
  })
  const assign = useMutation({
    mutationFn: (a: { tabId: TabId; folderId: TabFolderId | null }) =>
      commands.assignTabToFolder(a.tabId, a.folderId),
    onSuccess: invalidate,
  })
  const setCollapsed = useMutation({
    mutationFn: (a: { folderId: TabFolderId; collapsed: boolean }) =>
      commands.setTabFolderCollapsed(workspaceId, a.folderId, a.collapsed),
    onSuccess: invalidate,
  })
  const reorderFolders = useMutation({
    mutationFn: (order: TabFolderId[]) => commands.reorderTabFolders(workspaceId, order),
    onSuccess: invalidate,
  })
  const selectFolder = useMutation({
    // `selected` is the folder's *current* state; toggle deselect when it
    // is already the target, select it otherwise.
    mutationFn: (a: { folderId: TabFolderId; selected: boolean }) =>
      commands.selectTabFolder(workspaceId, a.selected ? null : a.folderId),
    onSuccess: invalidate,
  })
  const renameFolder = useMutation({
    mutationFn: (a: { folderId: TabFolderId; name: string }) =>
      commands.renameTabFolder(workspaceId, a.folderId, a.name),
    onSuccess: invalidate,
  })
  const { startTabDrag, draggedRef, openSplitMenu } = useTabSplit({
    workspaceId,
    activeTabId,
  })
  const { openIslandMenu } = useIslandMenu()

  const discardedTabs = useDiscardedTabs()
  const wrapRef = useRef<HTMLDivElement>(null)
  const stripRef = useRef<HTMLDivElement>(null)
  const activeRef = useRef<HTMLDivElement>(null)
  const [stripWidth, setStripWidth] = useState(0)
  const [dropTarget, setDropTarget] = useState<DropTarget | null>(null)
  const folderDropRef = useRef<TabFolderId | null | undefined>(undefined)
  const [folderDropHint, setFolderDropHint] = useState<string | null>(null)
  const pinDraggedRef = useRef(false)
  const [pinDropIndex, setPinDropIndex] = useState<number | null>(null)
  const folderReorderRef = useRef(false)
  const [folderReorderDrop, setFolderReorderDrop] = useState<number | null>(null)
  const [renamingId, setRenamingId] = useState<TabFolderId | null>(null)
  const [draft, setDraft] = useState('')
  const renameInputRef = useRef<HTMLInputElement>(null)
  // Set true right before an Escape-cancel so the input's blur skips saving.
  const cancelRenameRef = useRef(false)

  const beginRename = (folder: TabFolder) => {
    cancelRenameRef.current = false
    setDraft(folder.name)
    setRenamingId(folder.id)
    requestAnimationFrame(() => renameInputRef.current?.select())
  }
  const submitRename = (folderId: TabFolderId, originalName: string) => {
    if (cancelRenameRef.current) {
      cancelRenameRef.current = false
      setRenamingId(null)
      return
    }
    const trimmed = draft.trim()
    if (trimmed && trimmed !== originalName) renameFolder.mutate({ folderId, name: trimmed })
    setRenamingId(null)
  }

  const pinnedSet = new Set(pinnedTabIds)
  const pinnedTabs = tabs.filter((t) => pinnedSet.has(t.id))
  const stripTabs = tabs.filter((t) => !pinnedSet.has(t.id))

  // Sections: folders (islands) grouped together, then loose tabs.
  const folderGroups: FolderGroup[] = [...folders]
    .sort((a, b) => a.position - b.position)
    .map((folder) => ({ folder, members: stripTabs.filter((t) => t.folder_id === folder.id) }))
    .filter((g) => g.members.length > 0)
  const looseTabs = stripTabs.filter((t) => t.folder_id == null)
  const looseIds = new Set(looseTabs.map((t) => t.id))
  const sizableTabs: Tab[] = [...looseTabs]
  let collapsedCount = 0
  let expandedCount = 0
  for (const g of folderGroups) {
    if (g.folder.collapsed) collapsedCount += 1
    else {
      expandedCount += 1
      sizableTabs.push(...g.members)
    }
  }

  const makeReorder = (tab: Tab): TabReorder => ({
    targetAtPoint: (x, y) => {
      const fid = islandAt(x, y)
      if (fid !== undefined) {
        if (fid === tab.folder_id) {
          folderDropRef.current = undefined
          setFolderDropHint(null)
          return ISLAND_DROP
        }
        folderDropRef.current = fid
        setFolderDropHint(fid)
        return ISLAND_DROP
      }
      if (tab.folder_id != null && withinStrip(x, y)) {
        folderDropRef.current = null
        setFolderDropHint('loose')
        return ISLAND_DROP
      }
      folderDropRef.current = undefined
      setFolderDropHint(null)
      if (tab.folder_id == null && stripRef.current) {
        return tabDropTarget(stripRef.current, 'x', x, y)
      }
      return null
    },
    onTarget: setDropTarget,
    onDrop: (idx) => {
      const drop = folderDropRef.current
      folderDropRef.current = undefined
      setFolderDropHint(null)
      if (drop !== undefined) {
        if (drop !== (tab.folder_id ?? null)) {
          assign.mutate({ tabId: tab.id, folderId: drop })
        }
        return
      }
      if (tab.folder_id == null) {
        const next = reorderedAmong(
          tabs.map((t) => t.id),
          looseIds,
          tab.id,
          idx,
        )
        if (next) reorder.mutate(next)
      }
    },
  })

  const islandAt = (x: number, y: number): TabFolderId | undefined => {
    const root = stripRef.current
    if (!root) return undefined
    for (const el of root.querySelectorAll<HTMLElement>('[data-island-id]')) {
      const r = el.getBoundingClientRect()
      if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) {
        return el.dataset.islandId as TabFolderId
      }
    }
    return undefined
  }

  const withinStrip = (x: number, y: number): boolean => {
    const r = stripRef.current?.getBoundingClientRect()
    return !!r && x >= r.left && x <= r.right && y >= r.top && y <= r.bottom
  }

  // Reorder the pinned circles within the full tab order.
  const startPinDrag = (e: ReactPointerEvent, tabId: TabId) => {
    if (e.button !== 0) return
    const startX = e.clientX
    const startY = e.clientY
    const sourceEl = e.currentTarget as HTMLElement
    const rect = sourceEl.getBoundingClientRect()
    const grabX = startX - rect.left
    const grabY = startY - rect.top
    const fullOrder = tabs.map((t) => t.id)
    pinDraggedRef.current = false
    let begun = false
    let ghost: HTMLElement | null = null
    let lastIndex: number | null = null

    const insertionAt = (x: number, y: number): number => {
      const root = stripRef.current
      if (!root) return pinnedTabs.length
      const cells = Array.from(root.querySelectorAll<HTMLElement>('[data-pin-id]'))
      for (let i = 0; i < cells.length; i++) {
        const r = cells[i].getBoundingClientRect()
        if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) {
          return x < r.left + r.width / 2 ? i : i + 1
        }
      }
      return cells.length
    }

    const move = (ev: PointerEvent) => {
      if (!begun) {
        if (Math.hypot(ev.clientX - startX, ev.clientY - startY) <= 6) return
        begun = true
        pinDraggedRef.current = true
        document.body.classList.add('is-tab-dragging')
        window.getSelection()?.removeAllRanges()
        ghost = sourceEl.cloneNode(true) as HTMLElement
        ghost.classList.add('kb-drag-ghost')
        ghost.style.width = `${rect.width}px`
        ghost.style.height = `${rect.height}px`
        document.body.appendChild(ghost)
      }
      if (ghost) {
        ghost.style.left = `${ev.clientX - grabX}px`
        ghost.style.top = `${ev.clientY - grabY}px`
      }
      lastIndex = insertionAt(ev.clientX, ev.clientY)
      setPinDropIndex(lastIndex)
    }
    const up = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      ghost?.remove()
      if (begun) document.body.classList.remove('is-tab-dragging')
      setPinDropIndex(null)
      if (begun && lastIndex != null) {
        const next = reorderedAmong(fullOrder, pinnedSet, tabId, lastIndex)
        if (next) reorder.mutate(next)
      }
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  // Reorder folders (islands) by dragging their circle/name chip.
  const startFolderReorderDrag = (e: ReactPointerEvent, folderId: TabFolderId) => {
    if (e.button !== 0) return
    const startX = e.clientX
    const startY = e.clientY
    const sourceEl = e.currentTarget as HTMLElement
    const rect = sourceEl.getBoundingClientRect()
    const grabX = startX - rect.left
    const grabY = startY - rect.top
    const order = folderGroups.map((g) => g.folder.id)
    folderReorderRef.current = false
    let begun = false
    let ghost: HTMLElement | null = null
    let lastIndex: number | null = null

    const insertionAt = (x: number): number => {
      const root = stripRef.current
      if (!root) return order.length
      const els = Array.from(root.querySelectorAll<HTMLElement>('[data-island-id]'))
      for (let i = 0; i < els.length; i++) {
        const r = els[i].getBoundingClientRect()
        if (x < r.left + r.width / 2) return i
      }
      return els.length
    }

    const move = (ev: PointerEvent) => {
      if (!begun) {
        if (Math.hypot(ev.clientX - startX, ev.clientY - startY) <= 6) return
        begun = true
        folderReorderRef.current = true
        document.body.classList.add('is-tab-dragging')
        window.getSelection()?.removeAllRanges()
        ghost = sourceEl.cloneNode(true) as HTMLElement
        ghost.classList.add('kb-drag-ghost')
        ghost.style.width = `${rect.width}px`
        ghost.style.height = `${rect.height}px`
        document.body.appendChild(ghost)
      }
      if (ghost) {
        ghost.style.left = `${ev.clientX - grabX}px`
        ghost.style.top = `${ev.clientY - grabY}px`
      }
      lastIndex = insertionAt(ev.clientX)
      setFolderReorderDrop(lastIndex)
    }
    const up = () => {
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      ghost?.remove()
      if (begun) document.body.classList.remove('is-tab-dragging')
      setFolderReorderDrop(null)
      if (begun && lastIndex != null) {
        const next = moveInList(order, folderId, lastIndex)
        if (next) reorderFolders.mutate(next)
      }
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  useLayoutEffect(() => {
    const strip = stripRef.current
    if (!strip) return
    const update = () => setStripWidth(strip.clientWidth)
    update()
    const ro = new ResizeObserver(update)
    ro.observe(strip)
    return () => ro.disconnect()
  }, [])

  useLayoutEffect(() => {
    activeRef.current?.scrollIntoView({ block: 'nearest', inline: 'center', behavior: 'smooth' })
  }, [activeTabId])

  const minActive = compact ? COMPACT_MINIMUM_CURRENT_TAB_SIZE : MINIMUM_CURRENT_TAB_SIZE
  const minInactive = compact ? COMPACT_MINIMUM_TAB_SIZE : MINIMUM_TAB_SIZE
  const nameW = compact ? ISLAND_NAME_W_COMPACT : ISLAND_NAME_W

  if (folderGroups.length === 0 && looseTabs.length === 0 && pinnedTabs.length === 0) {
    return (
      <div className={cx('kb-tabs-h-wrap', { compact })} ref={wrapRef}>
        <div className={cx('tabs-h', { compact })} ref={stripRef} />
      </div>
    )
  }

  const n = sizableTabs.length
  const activeVisible = sizableTabs.some((t) => t.id === activeTabId)
  // Reserve the pinned circles, islands' fixed parts (circles, name chips +
  // pill padding), and every gap, so the flexible tabs fill exactly what's left.
  const expandedMembers = n - looseTabs.length
  const topLevelCount = pinnedTabs.length + folderGroups.length + looseTabs.length
  const totalGaps =
    Math.max(0, topLevelCount - 1) * GAP_PX + expandedMembers * GAP_PX
  const fixedWidth =
    pinnedTabs.length * PIN_CIRCLE_W +
    collapsedCount * ISLAND_CIRCLE_W +
    expandedCount * (nameW + ISLAND_PILL_PAD * 2)
  const available = Math.max(0, stripWidth - fixedWidth - totalGaps)
  const sizing = computeSizing(n, available, minActive, minInactive)

  const isMany = sizing.activeSize !== sizing.inactiveSize
  const inactiveCloseThreshold = compact ? 90 : 130
  const inactiveCompact = isMany && sizing.inactiveSize < inactiveCloseThreshold

  const sizeFor = (isActive: boolean) =>
    isActive && activeVisible ? sizing.activeSize : sizing.inactiveSize

  const renderTab = (tab: Tab, opts: { looseIndex?: number } = {}) => {
    const isActive = tab.id === activeTabId
    const loose = opts.looseIndex !== undefined
    return (
      <div
        key={tab.id}
        ref={isActive ? activeRef : undefined}
        {...(loose ? { 'data-tab-id': tab.id } : {})}
        role="button"
        tabIndex={0}
        className={cx('h-tab', {
          active: isActive,
          asleep: discardedTabs.has(tab.id) && !isActive,
          'as-urlbar': isActive && integrateUrlBar,
          'drop-before':
            loose && dropTarget?.anchorIndex === opts.looseIndex && dropTarget?.side === 'before',
          'drop-after':
            loose && dropTarget?.anchorIndex === opts.looseIndex && dropTarget?.side === 'after',
        })}
        style={{ flex: `0 0 ${sizeFor(isActive)}px` }}
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
        title={tab.title}
      >
        {isActive && integrateUrlBar ? (
          <UrlContent tab={tab} />
        ) : (
          <>
            <TabFavicon tab={tab} />
            <span className="ttl">{tabDisplayName(tab)}</span>
          </>
        )}
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

  const renderPinCircle = (tab: Tab, i: number) => (
    <button
      key={tab.id}
      type="button"
      data-pin-id={tab.id}
      className={cx('h-pin-circle', {
        active: tab.id === activeTabId,
        'drop-before': pinDropIndex === i,
        'drop-after': pinDropIndex === pinnedTabs.length && i === pinnedTabs.length - 1,
      })}
      title={tabDisplayName(tab)}
      aria-label={tabDisplayName(tab)}
      onPointerDown={(e) => startPinDrag(e, tab.id)}
      onClick={() => {
        if (pinDraggedRef.current) {
          pinDraggedRef.current = false
          return
        }
        setActive.mutate(tab.id)
      }}
      onContextMenu={(e) => openSplitMenu(e, tab.id)}
    >
      <TabFavicon tab={tab} />
    </button>
  )

  const renderFolder = ({ folder, members }: FolderGroup, index: number) => {
    const memberActive = members.some((m) => m.id === activeTabId)
    const isSelected = folder.id === selectedFolderId
    const isRenaming = renamingId === folder.id
    const isThemeAccent = folder.accent === themeAccent
    const islandTarget = {
      workspaceId,
      folderId: folder.id,
      accent: folder.accent,
      name: folder.name,
    }
    const reorderClasses = {
      'reorder-before': folderReorderDrop === index,
      'reorder-after':
        folderReorderDrop === folderGroups.length && index === folderGroups.length - 1,
    }
    const toggleCollapsed = (collapsed: boolean) => {
      if (folderReorderRef.current) {
        folderReorderRef.current = false
        return
      }
      setCollapsed.mutate({ folderId: folder.id, collapsed })
    }
    // IDE-style: clicking the folder selects it as the new-tab target; Enter on
    // a selected folder renames it (otherwise it selects). Mirrors the sidebar.
    const onFolderClick = () => {
      if (folderReorderRef.current) {
        folderReorderRef.current = false
        return
      }
      if (isRenaming) return
      selectFolder.mutate({ folderId: folder.id, selected: isSelected })
    }
    const onFolderKeyDown = (e: ReactKeyboardEvent) => {
      if (e.key !== 'Enter') return
      e.preventDefault()
      if (isSelected) beginRename(folder)
      else selectFolder.mutate({ folderId: folder.id, selected: false })
    }
    if (folder.collapsed) {
      return (
        <button
          key={folder.id}
          type="button"
          data-island-id={folder.id}
          className={cx('h-island-circle', `accent-${folder.accent}`, {
            active: memberActive,
            selected: isSelected,
            'ring-contrast': isThemeAccent,
            'is-drop': folderDropHint === folder.id,
            ...reorderClasses,
          })}
          title={`${folder.name} (${members.length})`}
          aria-label={`${folder.name}, ${members.length} tabs`}
          onPointerDown={(e) => startFolderReorderDrag(e, folder.id)}
          onClick={() => toggleCollapsed(false)}
          onContextMenu={(e) => openIslandMenu(e, islandTarget)}
        />
      )
    }
    const memberSum = members.reduce((s, m) => s + sizeFor(m.id === activeTabId), 0)
    const islandW = nameW + memberSum + members.length * GAP_PX + ISLAND_PILL_PAD * 2
    return (
      <div
        key={folder.id}
        data-island-id={folder.id}
        className={cx('h-island', { 'is-drop': folderDropHint === folder.id, ...reorderClasses })}
        style={{ flex: `0 0 ${islandW}px` }}
      >
        <div
          className={cx('h-island-name', `accent-${folder.accent}`, {
            selected: isSelected,
            'ring-contrast': isThemeAccent,
          })}
          style={{ flex: `0 0 ${nameW}px` }}
          role="button"
          tabIndex={0}
          aria-pressed={isSelected}
          title={folder.name}
          onPointerDown={(e) => startFolderReorderDrag(e, folder.id)}
          onClick={onFolderClick}
          onDoubleClick={(e) => {
            e.stopPropagation()
            beginRename(folder)
          }}
          onKeyDown={onFolderKeyDown}
          onContextMenu={(e) => openIslandMenu(e, islandTarget)}
        >
          <button
            type="button"
            className="h-island-chevron"
            aria-label="Collapse folder"
            title="Collapse folder"
            onPointerDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation()
              toggleCollapsed(true)
            }}
          >
            <CollapseChevron />
          </button>
          {isRenaming ? (
            <input
              ref={renameInputRef}
              className="h-island-rename"
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onPointerDown={(e) => e.stopPropagation()}
              onClick={(e) => e.stopPropagation()}
              onBlur={() => submitRename(folder.id, folder.name)}
              onKeyDown={(e) => {
                e.stopPropagation()
                if (e.key === 'Enter') submitRename(folder.id, folder.name)
                else if (e.key === 'Escape') {
                  cancelRenameRef.current = true
                  setRenamingId(null)
                }
              }}
            />
          ) : (
            <span className="h-island-label">{folder.name}</span>
          )}
        </div>
        {members.map((tab) => renderTab(tab))}
      </div>
    )
  }

  const pinnedEls = pinnedTabs.map((tab, i) => renderPinCircle(tab, i))
  const folderEls = folderGroups.map((g, i) => renderFolder(g, i))
  const looseEls = looseTabs.map((tab, i) => renderTab(tab, { looseIndex: i }))
  // Rail-side ordering: pinned sits nearest the rail. Left rail →
  // pinned | folders | loose; right rail → loose | folders | pinned.
  const ordered =
    railSide === 'right'
      ? [...looseEls, ...folderEls, ...pinnedEls]
      : [...pinnedEls, ...folderEls, ...looseEls]

  return (
    <div
      className={cx('kb-tabs-h-wrap', {
        compact,
        'is-many': isMany,
        'is-scrolling': sizing.scrolling,
        'is-inactive-compact': inactiveCompact,
      })}
      ref={wrapRef}
    >
      <div className={cx('tabs-h', { compact })} ref={stripRef}>
        {ordered}
      </div>
    </div>
  )
}

/** Small leftward chevron on the expanded island chip: collapses to a circle. */
function CollapseChevron() {
  return (
    <svg
      width="11"
      height="11"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="3"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="m15 18-6-6 6-6" />
    </svg>
  )
}

function UrlContent({ tab }: { tab: Tab }) {
  const parts = (() => {
    try {
      const u = new URL(tab.url)
      if (u.protocol === 'about:' || u.host === '') {
        return null
      }
      return { domain: u.host, path: u.pathname === '/' ? '' : u.pathname }
    } catch {
      return null
    }
  })()

  return (
    <>
      <LockIcon className="ico" />
      {parts ? (
        <>
          <span className="domain">{parts.domain}</span>
          <span className="path">{parts.path}</span>
        </>
      ) : (
        <span className="path">Search or enter URL</span>
      )}
    </>
  )
}
