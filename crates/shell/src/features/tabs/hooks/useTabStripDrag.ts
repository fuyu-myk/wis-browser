import {
  useRef,
  useState,
  type MutableRefObject,
  type PointerEvent as ReactPointerEvent,
  type RefObject,
} from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { Tab, TabFolderId, TabId, WorkspaceId } from '@/bindings/types'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'
import {
  patchFolderOrder,
  patchTabFolder,
  patchTabOrder,
  type PendingGlide,
} from '@/features/tabs/lib/tabMotion'
import {
  useTabSplit,
  tabDropTarget,
  layoutRect,
  insertedAmong,
  reorderedAmong,
  type TabReorder,
  type DropTarget,
} from '@/features/tabs/hooks/useTabSplit'
import { GAP_PX, moveInList, type FolderGroup } from '@/features/tabs/lib/tabStripLayout'

/** A non-reorder drop target: "assign to island", not "insert here". */
const ISLAND_DROP: DropTarget = { index: -1, anchorIndex: -1, side: 'before' }

interface TabStripDragArgs {
  workspaceId: WorkspaceId
  activeTabId: TabId | null
  stripRef: RefObject<HTMLDivElement>
  /** Full tab list in order, for building the reorder payload. */
  tabs: Tab[]
  looseTabs: Tab[]
  looseIds: Set<TabId>
  pinnedTabs: Tab[]
  pinnedSet: Set<TabId>
  folderGroups: FolderGroup[]
}

interface TabStripDrag {
  startTabDrag: ReturnType<typeof useTabSplit>['startTabDrag']
  draggedRef: ReturnType<typeof useTabSplit>['draggedRef']
  openSplitMenu: ReturnType<typeof useTabSplit>['openSplitMenu']
  makeReorder: (tab: Tab) => TabReorder
  startPinDrag: (e: ReactPointerEvent, tabId: TabId) => void
  pinDraggedRef: MutableRefObject<boolean>
  pinDragId: TabId | null
  pinDropIndex: number | null
  startFolderReorderDrag: (e: ReactPointerEvent, folderId: TabFolderId) => void
  folderReorderRef: MutableRefObject<boolean>
  folderReorderDrop: number | null
  islandDragPart: { id: TabFolderId; size: number } | null
  dropTarget: DropTarget | null
  dragPart: { id: TabId; size: number } | null
  dragScopeRef: RefObject<string | null>
  folderDropHint: string | null
  pendingGlideRef: RefObject<PendingGlide | null>
  reorderTabs: (order: TabId[]) => void
  reorderFolders: (order: TabFolderId[]) => void
}

/**
 * The horizontal tab strip's three pointer-drag systems: reorder/file a tab
 * (via `useTabSplit`), reorder the pinned circles, and reorder folder
 * islands. Owns the live drag state the strip render reads back (drop target,
 * parting sizes, drop hints) plus the optimistic reorder mutations, keeping
 * the component itself to derivation + render.
 */
export function useTabStripDrag(d: TabStripDragArgs): TabStripDrag {
  const qc = useQueryClient()
  const invalidate = () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY })
  const reorder = useMutation({
    mutationFn: (order: TabId[]) => commands.reorderTabs(d.workspaceId, order),
    // Optimistic: drops re-render in the new order on the same frame, so the
    // glide-into-place animation has its final slot to land in.
    onMutate: (order) => patchTabOrder(qc, d.workspaceId, order),
    onSuccess: invalidate,
  })
  const assign = useMutation({
    mutationFn: (a: { tabId: TabId; folderId: TabFolderId | null }) =>
      commands.assignTabToFolder(a.tabId, a.folderId),
    // Optimistic: filing renders on the drop frame so the glide can land.
    onMutate: (a) => patchTabFolder(qc, a.tabId, a.folderId),
    onSuccess: invalidate,
  })
  const reorderFolders = useMutation({
    mutationFn: (order: TabFolderId[]) => commands.reorderTabFolders(d.workspaceId, order),
    onMutate: (order) => patchFolderOrder(qc, d.workspaceId, order),
    onSuccess: invalidate,
  })
  const { startTabDrag, draggedRef, openSplitMenu } = useTabSplit({
    workspaceId: d.workspaceId,
    activeTabId: d.activeTabId,
  })

  const [dropTarget, setDropTarget] = useState<DropTarget | null>(null)
  const folderDropRef = useRef<TabFolderId | null | undefined>(undefined)
  const [folderDropHint, setFolderDropHint] = useState<string | null>(null)
  // Which list a positional tab reorder targets: 'loose' or a folder id.
  const dragScopeRef = useRef<string | null>(null)
  const pinDraggedRef = useRef(false)
  const [pinDropIndex, setPinDropIndex] = useState<number | null>(null)
  const folderReorderRef = useRef(false)
  const [folderReorderDrop, setFolderReorderDrop] = useState<number | null>(null)
  // Live parting state per drag kind.
  const [dragPart, setDragPart] = useState<{ id: TabId; size: number } | null>(null)
  const [pinDragId, setPinDragId] = useState<TabId | null>(null)
  const [islandDragPart, setIslandDragPart] = useState<{ id: TabFolderId; size: number } | null>(
    null,
  )
  const pendingGlideRef = useRef<PendingGlide | null>(null)

  const islandAt = (x: number, y: number): TabFolderId | undefined => {
    const root = d.stripRef.current
    if (!root) return undefined
    for (const el of root.querySelectorAll<HTMLElement>('[data-island-id]')) {
      const r = layoutRect(el)
      if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) {
        return el.dataset.islandId as TabFolderId
      }
    }
    return undefined
  }

  const withinStrip = (x: number, y: number): boolean => {
    const r = d.stripRef.current?.getBoundingClientRect()
    return !!r && x >= r.left && x <= r.right && y >= r.top && y <= r.bottom
  }

  // Whether the pointer is horizontally over the islands/pinned block.
  const overFixedCells = (x: number): boolean => {
    const root = d.stripRef.current
    if (!root) return false
    for (const el of root.querySelectorAll<HTMLElement>('[data-island-id], [data-pin-id]')) {
      const r = layoutRect(el)
      if (x >= r.left - GAP_PX && x <= r.right + GAP_PX) return true
    }
    return false
  }

  const makeReorder = (tab: Tab): TabReorder => ({
    targetAtPoint: (x, y) => {
      const fid = islandAt(x, y)
      if (fid !== undefined) {
        // Expanded island.
        const group = d.folderGroups.find((g) => g.folder.id === fid)
        if (group !== undefined && !group.folder.collapsed) {
          const islandEl = d.stripRef.current?.querySelector<HTMLElement>(
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
        const t =
          d.stripRef.current && !overFixedCells(x)
            ? tabDropTarget(d.stripRef.current, 'x', x, y)
            : null
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
      if (tab.folder_id == null && d.stripRef.current) {
        if (overFixedCells(x)) {
          dragScopeRef.current = null
          return null
        }
        dragScopeRef.current = 'loose'
        return tabDropTarget(d.stripRef.current, 'x', x, y)
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
      const el = d.stripRef.current?.querySelector<HTMLElement>(
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
      const fullOrder = d.tabs.map((t) => t.id)
      if (scope === 'loose') {
        if (tab.folder_id == null) {
          const next = reorderedAmong(fullOrder, d.looseIds, tab.id, idx)
          if (next) {
            glide(`[data-tab-id="${tab.id}"]`)
            reorder.mutate(next)
          }
        } else {
          // Positional unfile into the loose strip.
          const next = insertedAmong(fullOrder, d.looseIds, tab.id, idx)
          glide(`[data-tab-id="${tab.id}"]`)
          assign.mutate({ tabId: tab.id, folderId: null })
          if (next) reorder.mutate(next)
        }
        return
      }
      if (scope) {
        const folderId = scope as TabFolderId
        const memberIds = new Set(
          (d.folderGroups.find((g) => g.folder.id === folderId)?.members ?? []).map((m) => m.id),
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

  // Reorder the pinned circles within the full tab order.
  const startPinDrag = (e: ReactPointerEvent, tabId: TabId) => {
    if (e.button !== 0) return
    const startX = e.clientX
    const startY = e.clientY
    const sourceEl = e.currentTarget as HTMLElement
    const rect = sourceEl.getBoundingClientRect()
    const grabX = startX - rect.left
    const grabY = startY - rect.top
    const fullOrder = d.tabs.map((t) => t.id)
    pinDraggedRef.current = false
    let begun = false
    let ghost: HTMLElement | null = null
    let lastIndex: number | null = null

    const insertionAt = (x: number): number => {
      const root = d.stripRef.current
      if (!root) return d.pinnedTabs.length
      let idx = 0
      for (const el of root.querySelectorAll<HTMLElement>('[data-pin-id]')) {
        const r = layoutRect(el)
        if (x > r.left + r.width / 2) idx += 1
      }
      return idx
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
      lastIndex = insertionAt(ev.clientX)
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
        const next = reorderedAmong(fullOrder, d.pinnedSet, tabId, lastIndex)
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
    const order = d.folderGroups.map((g) => g.folder.id)
    folderReorderRef.current = false
    let begun = false
    let ghost: HTMLElement | null = null
    let lastIndex: number | null = null

    const insertionAt = (x: number): number => {
      const root = d.stripRef.current
      if (!root) return order.length
      const els = Array.from(root.querySelectorAll<HTMLElement>('[data-island-id]'))
      for (let i = 0; i < els.length; i++) {
        const r = layoutRect(els[i])
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
        const island = d.stripRef.current?.querySelector<HTMLElement>(
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

  return {
    startTabDrag,
    draggedRef,
    openSplitMenu,
    makeReorder,
    startPinDrag,
    pinDraggedRef,
    pinDragId,
    pinDropIndex,
    startFolderReorderDrag,
    folderReorderRef,
    folderReorderDrop,
    islandDragPart,
    dropTarget,
    dragPart,
    dragScopeRef,
    folderDropHint,
    pendingGlideRef,
    reorderTabs: (order) => reorder.mutate(order),
    reorderFolders: (order) => reorderFolders.mutate(order),
  }
}
