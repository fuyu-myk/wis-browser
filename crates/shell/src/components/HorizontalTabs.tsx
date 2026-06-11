import { useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
} from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'

const ISLAND_EXIT_MS = 200

import { commands } from '@/bindings/commands'
import { closeTabAnimated, onTabClosing, TAB_EXIT_MS } from '@/lib/tabClose'
import type {
  AccentColor,
  BrowserState,
  RailSide,
  Tab,
  TabFolder,
  TabFolderId,
  TabId,
  WorkspaceId,
} from '@/bindings/types'
import { cx } from '@/lib/cx'
import { renderIconRef } from '@/lib/iconGlyphs'
import { LockIcon, XIcon } from '@/lib/icons'
import { tabDisplayName } from '@/lib/url'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'
import {
  partOffset,
  patchFolderOrder,
  patchTabFolder,
  patchTabOrder,
  runPendingGlide,
  type PendingGlide,
} from '@/lib/tabMotion'
import {
  useTabSplit,
  tabDropTarget,
  insertedAmong,
  reorderedAmong,
  type TabReorder,
  type DropTarget,
} from '@/hooks/useTabSplit'

import { useFolderTarget } from '@/hooks/useFolderTarget'
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
    // Optimistic: drops re-render in the new order on the same frame, so the
    // glide-into-place animation has its final slot to land in.
    onMutate: (order) => patchTabOrder(qc, workspaceId, order),
    onSuccess: invalidate,
  })
  const assign = useMutation({
    mutationFn: (a: { tabId: TabId; folderId: TabFolderId | null }) =>
      commands.assignTabToFolder(a.tabId, a.folderId),
    // Optimistic: filing renders on the drop frame so the glide can land.
    onMutate: (a) => patchTabFolder(qc, a.tabId, a.folderId),
    onSuccess: invalidate,
  })
  const setCollapsed = useMutation({
    mutationFn: (a: { folderId: TabFolderId; collapsed: boolean }) =>
      commands.setTabFolderCollapsed(workspaceId, a.folderId, a.collapsed),
    // Optimistic: the island and the strip reflow on this frame instead of
    // after the IPC round trip.
    onMutate: (a) => {
      qc.setQueryData<BrowserState>([...BROWSER_STATE_KEY], (old) =>
        old
          ? {
              ...old,
              workspaces: old.workspaces.map((ws) =>
                ws.id === workspaceId
                  ? {
                      ...ws,
                      tab_folders: ws.tab_folders.map((f) =>
                        f.id === a.folderId ? { ...f, collapsed: a.collapsed } : f,
                      ),
                    }
                  : ws,
              ),
            }
          : old,
      )
    },
    onSuccess: invalidate,
  })
  const reorderFolders = useMutation({
    mutationFn: (order: TabFolderId[]) => commands.reorderTabFolders(workspaceId, order),
    onMutate: (order) => patchFolderOrder(qc, workspaceId, order),
    onSuccess: invalidate,
  })
  const { setTarget, syncToTab } = useFolderTarget(workspaceId, selectedFolderId)
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
  // Which list a positional tab reorder targets: 'loose' or a folder id.
  const dragScopeRef = useRef<string | null>(null)
  const pinDraggedRef = useRef(false)
  const [pinDropIndex, setPinDropIndex] = useState<number | null>(null)
  const folderReorderRef = useRef(false)
  const [folderReorderDrop, setFolderReorderDrop] = useState<number | null>(null)
  const [renamingId, setRenamingId] = useState<TabFolderId | null>(null)
  const [closingIslandId, setClosingIslandId] = useState<TabFolderId | null>(null)
  // Live parting state per drag kind.
  const [dragPart, setDragPart] = useState<{ id: TabId; size: number } | null>(null)
  const [pinDragId, setPinDragId] = useState<TabId | null>(null)
  const [islandDragPart, setIslandDragPart] = useState<{
    id: TabFolderId
    size: number
  } | null>(null)
  const pendingGlideRef = useRef<PendingGlide | null>(null)
  useLayoutEffect(() => {
    runPendingGlide(stripRef.current, pendingGlideRef)
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
    if (g.folder.collapsed || g.folder.id === closingIslandId) collapsedCount += 1
    else {
      expandedCount += 1
      sizableTabs.push(...g.members)
    }
  }

  const makeReorder = (tab: Tab): TabReorder => ({
    targetAtPoint: (x, y) => {
      const fid = islandAt(x, y)
      if (fid !== undefined) {
        // Expanded island.
        const group = folderGroups.find((g) => g.folder.id === fid)
        if (group !== undefined && !group.folder.collapsed) {
          const islandEl = stripRef.current?.querySelector<HTMLElement>(
            `[data-island-id="${fid}"]`,
          )
          const t = islandEl ? tabDropTarget(islandEl, 'x', x, y, '[data-mtab-id]') : null
          if (t) {
            folderDropRef.current = undefined
            setFolderDropHint(null)
            dragScopeRef.current = fid
            return t
          }
        }
        dragScopeRef.current = null
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
        // Member over the open strip.
        const t = stripRef.current ? tabDropTarget(stripRef.current, 'x', x, y) : null
        if (t) {
          folderDropRef.current = undefined
          setFolderDropHint(null)
          dragScopeRef.current = 'loose'
          return t
        }
        folderDropRef.current = null
        setFolderDropHint('loose')
        dragScopeRef.current = null
        return ISLAND_DROP
      }
      folderDropRef.current = undefined
      setFolderDropHint(null)
      if (tab.folder_id == null && stripRef.current) {
        dragScopeRef.current = 'loose'
        return tabDropTarget(stripRef.current, 'x', x, y)
      }
      dragScopeRef.current = null
      return null
    },
    onTarget: setDropTarget,
    onDragChange: (dragging) => {
      if (!dragging) {
        setDragPart(null)
        return
      }
      const el = stripRef.current?.querySelector<HTMLElement>(
        `[data-tab-id="${tab.id}"], [data-mtab-id="${tab.id}"]`,
      )
      setDragPart({ id: tab.id, size: (el?.getBoundingClientRect().width ?? 120) + GAP_PX })
    },
    onDrop: (idx, ghostRect) => {
      const drop = folderDropRef.current
      const scope = dragScopeRef.current
      folderDropRef.current = undefined
      setFolderDropHint(null)
      dragScopeRef.current = null
      const glide = (selector: string) => {
        if (ghostRect) pendingGlideRef.current = { selector, rect: ghostRect }
      }
      if (drop !== undefined) {
        if (drop !== (tab.folder_id ?? null)) {
          assign.mutate({ tabId: tab.id, folderId: drop })
        }
        return
      }
      const fullOrder = tabs.map((t) => t.id)
      if (scope === 'loose') {
        if (tab.folder_id == null) {
          const next = reorderedAmong(fullOrder, looseIds, tab.id, idx)
          if (next) {
            glide(`[data-tab-id="${tab.id}"]`)
            reorder.mutate(next)
          }
        } else {
          // Positional unfile into the loose strip.
          const next = insertedAmong(fullOrder, looseIds, tab.id, idx)
          glide(`[data-tab-id="${tab.id}"]`)
          assign.mutate({ tabId: tab.id, folderId: null })
          if (next) reorder.mutate(next)
        }
        return
      }
      if (scope) {
        const folderId = scope as TabFolderId
        const memberIds = new Set(
          (folderGroups.find((g) => g.folder.id === folderId)?.members ?? []).map((m) => m.id),
        )
        if (tab.folder_id === folderId) {
          const next = reorderedAmong(fullOrder, memberIds, tab.id, idx)
          if (next) {
            glide(`[data-mtab-id="${tab.id}"]`)
            reorder.mutate(next)
          }
        } else {
          // File into the open island at the targeted slot.
          const next = insertedAmong(fullOrder, memberIds, tab.id, idx)
          glide(`[data-mtab-id="${tab.id}"]`)
          assign.mutate({ tabId: tab.id, folderId })
          if (next) reorder.mutate(next)
        }
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
        setPinDragId(tabId)
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
      const ghostRect = ghost?.getBoundingClientRect()
      ghost?.remove()
      if (begun) document.body.classList.remove('is-tab-dragging')
      setPinDropIndex(null)
      setPinDragId(null)
      if (begun && lastIndex != null) {
        const next = reorderedAmong(fullOrder, pinnedSet, tabId, lastIndex)
        if (next) {
          if (ghostRect) {
            pendingGlideRef.current = { selector: `[data-pin-id="${tabId}"]`, rect: ghostRect }
          }
          reorder.mutate(next)
        }
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
        const island = stripRef.current?.querySelector<HTMLElement>(
          `[data-island-id="${folderId}"]`,
        )
        setIslandDragPart({
          id: folderId,
          size: (island?.getBoundingClientRect().width ?? rect.width) + GAP_PX,
        })
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
      const ghostRect = ghost?.getBoundingClientRect()
      ghost?.remove()
      if (begun) document.body.classList.remove('is-tab-dragging')
      setFolderReorderDrop(null)
      setIslandDragPart(null)
      if (begun && lastIndex != null) {
        const next = moveInList(order, folderId, lastIndex)
        if (next) {
          if (ghostRect) {
            pendingGlideRef.current = {
              selector: `[data-island-id="${folderId}"]`,
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

  const renderTab = (
    tab: Tab,
    opts: {
      looseIndex?: number
      memberOf?: TabFolderId
      memberIndex?: number
      memberFrom?: number
    } = {},
  ) => {
    const isActive = tab.id === activeTabId
    const loose = opts.looseIndex !== undefined
    const member = opts.memberOf !== undefined
    const scope = dragScopeRef.current
    const isSource = dragPart?.id === tab.id
    // Optimistic resize.
    const lifted = isSource && scope !== (member ? opts.memberOf : loose ? 'loose' : null)
    let shift = 0
    if (dragPart && dropTarget !== null && dropTarget.index >= 0) {
      if (loose && scope === 'loose') {
        const from = looseTabs.findIndex((t) => t.id === dragPart.id)
        shift = partOffset(opts.looseIndex ?? -1, from, dropTarget.index, railSide === 'right' ? 1 : -1)
      } else if (member && scope === opts.memberOf) {
        shift = partOffset(opts.memberIndex ?? -1, opts.memberFrom ?? -1, dropTarget.index)
      }
    }
    return (
      <div
        key={tab.id}
        ref={isActive ? activeRef : undefined}
        {...(loose ? { 'data-tab-id': tab.id } : {})}
        {...(member ? { 'data-mtab-id': tab.id } : {})}
        role="button"
        tabIndex={0}
        className={cx('h-tab', {
          'is-closing': closingIds.has(tab.id),
          active: isActive,
          asleep: discardedTabs.has(tab.id) && !isActive,
          'as-urlbar': isActive && integrateUrlBar,
          'is-dragsource': isSource,
        })}
        style={{
          flex: lifted ? '0 0 0px' : `0 0 ${sizeFor(isActive)}px`,
          ...(lifted ? { marginLeft: `${-GAP_PX}px` } : {}),
          ...(shift !== 0 && dragPart
            ? { transform: `translateX(${shift * dragPart.size}px)` }
            : {}),
        }}
        onPointerDown={(e) => startTabDrag(e, tab.id, makeReorder(tab))}
        onClick={() => {
          if (draggedRef.current) {
            draggedRef.current = false
            return
          }
          setActive.mutate(tab.id)
          syncToTab(tab)
        }}
        onContextMenu={(e) => openSplitMenu(e, tab.id)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault()
            setActive.mutate(tab.id)
            syncToTab(tab)
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
            closeTabAnimated(tab.id)
          }}
        >
          <XIcon />
        </button>
      </div>
    )
  }

  const renderPinCircle = (tab: Tab, i: number) => {
    const pinFrom = pinDragId ? pinnedTabs.findIndex((t) => t.id === pinDragId) : -1
    const shift =
      pinDragId && pinDropIndex !== null && pinFrom >= 0
        ? partOffset(i, pinFrom, pinDropIndex)
        : 0
    return (
    <button
      key={tab.id}
      type="button"
      data-pin-id={tab.id}
      className={cx('h-pin-circle', {
        active: tab.id === activeTabId,
        'is-dragsource': pinDragId === tab.id,
      })}
      style={
        shift !== 0 ? { transform: `translateX(${shift * (PIN_CIRCLE_W + GAP_PX)}px)` } : undefined
      }
      title={tabDisplayName(tab)}
      aria-label={tabDisplayName(tab)}
      onPointerDown={(e) => startPinDrag(e, tab.id)}
      onClick={() => {
        if (pinDraggedRef.current) {
          pinDraggedRef.current = false
          return
        }
        setActive.mutate(tab.id)
        syncToTab(tab)
      }}
      onContextMenu={(e) => openSplitMenu(e, tab.id)}
    >
      <TabFavicon tab={tab} />
    </button>
    )
  }

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
    const islandFrom = islandDragPart
      ? folderGroups.findIndex((g) => g.folder.id === islandDragPart.id)
      : -1
    const islandShift =
      islandDragPart && folderReorderDrop !== null && islandFrom >= 0
        ? partOffset(index, islandFrom, folderReorderDrop)
        : 0
    const islandStyle =
      islandShift !== 0 && islandDragPart
        ? { transform: `translateX(${islandShift * islandDragPart.size}px)` }
        : undefined
    const isIslandSource = islandDragPart?.id === folder.id
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
      setTarget(isSelected ? null : folder.id)
    }
    const onFolderKeyDown = (e: ReactKeyboardEvent) => {
      if (e.key !== 'Enter') return
      e.preventDefault()
      if (isSelected) beginRename(folder)
      else setTarget(folder.id)
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
            'is-dragsource': isIslandSource,
          })}
          style={islandStyle}
          title={`${folder.name} (${members.length})`}
          aria-label={`${folder.name}, ${members.length} tabs`}
          onPointerDown={(e) => startFolderReorderDrag(e, folder.id)}
          onClick={() => toggleCollapsed(false)}
          onContextMenu={(e) => openIslandMenu(e, islandTarget)}
        >
          {renderIconRef(folder.icon, 11)}
        </button>
      )
    }
    const closing = closingIslandId === folder.id
    const memberFrom = dragPart ? members.findIndex((m) => m.id === dragPart.id) : -1
    // Optimistic resize.
    const liftedOut =
      memberFrom >= 0 && dragPart !== null && dragScopeRef.current !== folder.id
    const sizedMembers = liftedOut
      ? members.filter((m) => m.id !== dragPart?.id)
      : members
    const memberSum = sizedMembers.reduce((s, m) => s + sizeFor(m.id === activeTabId), 0)
    const externalTarget =
      dragPart !== null &&
      memberFrom < 0 &&
      dragScopeRef.current === folder.id &&
      dropTarget !== null &&
      dropTarget.index >= 0
    const islandW = closing
      ? ISLAND_CIRCLE_W
      : nameW +
        memberSum +
        sizedMembers.length * GAP_PX +
        ISLAND_PILL_PAD * 2 +
        (externalTarget && dragPart ? dragPart.size : 0)
    return (
      <div
        key={folder.id}
        data-island-id={folder.id}
        className={cx('h-island', {
          'is-drop': folderDropHint === folder.id,
          'is-closing': closingIslandId === folder.id,
          'is-dragsource': isIslandSource,
        })}
        style={{ flex: `0 0 ${islandW}px`, ...(islandStyle ?? {}) }}
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
              setClosingIslandId(folder.id)
              window.setTimeout(() => {
                toggleCollapsed(true)
                setClosingIslandId(null)
              }, ISLAND_EXIT_MS)
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
            <span className="h-island-label">
              {renderIconRef(folder.icon, 11)}
              {folder.name}
            </span>
          )}
        </div>
        {members.map((tab, i) =>
          renderTab(tab, { memberOf: folder.id, memberIndex: i, memberFrom }),
        )}
      </div>
    )
  }

  const onStripClick = (e: ReactMouseEvent) => {
    if (selectedFolderId == null) return
    // Tabs, pinned tabs and folder islands set the target themselves; only a
    // click on bare strip background clears it.
    if ((e.target as Element).closest('.h-tab, .h-pin-circle, .h-island, .h-island-circle')) return
    setTarget(null)
  }

  const pinnedEls = pinnedTabs.map((tab, i) => renderPinCircle(tab, i))
  const folderEls = folderGroups.map((g, i) => renderFolder(g, i))
  const looseEls = looseTabs.map((tab, i) => renderTab(tab, { looseIndex: i }))
  // Loose tabs live in their own flex item sized by the container, so their
  // width animations can never push the folders and pinned circles around.
  const looseGroup =
    looseEls.length > 0 ? (
      <div className="h-loose-group" key="loose-group">
        {looseEls}
      </div>
    ) : null
  // Rail-side ordering: pinned sits nearest the rail. Left rail →
  // pinned | folders | loose; right rail → loose | folders | pinned.
  const ordered =
    railSide === 'right'
      ? [looseGroup, ...folderEls, ...pinnedEls]
      : [...pinnedEls, ...folderEls, looseGroup]

  return (
    <div
      className={cx('kb-tabs-h-wrap', {
        compact,
        'is-many': isMany,
        'is-scrolling': sizing.scrolling,
        'is-inactive-compact': inactiveCompact,
      })}
      ref={wrapRef}
      onClick={onStripClick}
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
