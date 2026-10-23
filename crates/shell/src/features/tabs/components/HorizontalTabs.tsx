import { useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
} from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import { onTabClosing, TAB_EXIT_MS } from '@/features/tabs/lib/tabClose'
import { closeReturnTarget, noteTabClosed, recentTabs } from '@/features/tabs/lib/tabMru'
import {
  COMPACT_MEMBER_TAB_MAX,
  COMPACT_MINIMUM_CURRENT_TAB_SIZE,
  COMPACT_MINIMUM_TAB_SIZE,
  GAP_PX,
  ISLAND_CIRCLE_W,
  ISLAND_EXIT_MS,
  ISLAND_PILL_PAD,
  MEMBER_TAB_MAX,
  MINIMUM_CURRENT_TAB_SIZE,
  MINIMUM_TAB_SIZE,
  PIN_CIRCLE_W,
  chipWidthFor,
  computeStripSizing,
  type FolderGroup,
} from '@/features/tabs/lib/tabStripLayout'
import type {
  AccentColor,
  BrowserState,
  PaneNode,
  RailSide,
  Tab,
  TabFolder,
  TabFolderId,
  TabId,
  WorkspaceId,
} from '@/bindings/types'
import { predictActiveAfterClose } from '@/features/tabs/lib/paneActive'
import { cx } from '@/lib/cx'
import { renderIconRef } from '@/features/workspaces/lib/iconGlyphs'
import { XIcon } from '@/lib/icons'
import { tabDisplayName } from '@/lib/url'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'
import { partOffset, runPendingGlide } from '@/features/tabs/lib/tabMotion'
import { useWorkspaceSlide } from '@/features/tabs/hooks/useWorkspaceSlide'
import { useTabStripDrag } from '@/features/tabs/hooks/useTabStripDrag'

import { useFolderTarget } from '@/features/tabs/hooks/useFolderTarget'
import { useIslandMenu } from '@/features/tabs/hooks/useIslandMenu'
import { useTabAudio } from '@/features/tabs/hooks/useTabAudio'
import { TabAudioButton } from '@/features/tabs/components/TabAudioButton'
import { TabFavicon } from '@/features/tabs/components/TabFavicon'
import { useDiscardedTabs } from '@/features/tabs/components/DiscardedTabsContext'
import { CollapseChevron, UrlContent } from '@/features/tabs/components/HorizontalTabsParts'

/** Marks the close-FLIP glide animations so a rapid re-close can cancel them. */
const FLIP_ANIM_ID = 'wis-tab-flip'

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
  /** Workspace ids in display order, for the directional switch slide. */
  workspaceOrder?: WorkspaceId[]
  /** Rail edge: drives section order, where pinned sits nearest to the rail. */
  railSide: RailSide
  /** The active theme accent; a folder using it needs a contrast active ring. */
  themeAccent: AccentColor
  /** When true, render the URL inside the active tab. */
  integrateUrlBar: boolean
  /** Render as a smaller, centered tab strip. */
  compact?: boolean
  /**
   * Max islands expanded at once: expanding past the limit collapses the
   * least-recently-expanded (accordion). 0 means no limit.
   */
  maxExpandedFolders?: number
  /**
   * The active workspace's split pane tree (null when unsplit). Used to predict
   * which tab the backend promotes when the active tab closes, so the strip can
   * grow + highlight it immediately (a split promotes a pane leaf rather than
   * the tab-order neighbour).
   */
  paneTree?: PaneNode | null
}

/**
 * Horizontal tab strip. Loose tabs flex to share the row; live-tab folders
 * render as "islands": a small colored circle when collapsed, or a glass pill
 * (colored name + its tabs) when expanded. Islands take a fixed slice of the
 * row; the rest is shared by the loose and expanded-island tabs.
 */
export function HorizontalTabs({
  workspaceId,
  tabs,
  activeTabId,
  pinnedTabIds,
  workspaceOrder,
  folders,
  selectedFolderId,
  railSide,
  themeAccent,
  integrateUrlBar,
  compact = false,
  maxExpandedFolders = 0,
  paneTree = null,
}: HorizontalTabsProps) {
  const qc = useQueryClient()
  const invalidate = () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY })
  const tabAudio = useTabAudio()
  const setActive = useMutation({
    mutationFn: (id: TabId) => commands.setActiveTab(id),
    onSuccess: invalidate,
  })
  const close = useMutation({
    mutationFn: (id: TabId) => {
      const returnTo = closeReturnTarget(id)
      noteTabClosed(id)
      return commands.closeTab(id, returnTo, recentTabs())
    },
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
  const { setTarget } = useFolderTarget(workspaceId)
  const renameFolder = useMutation({
    mutationFn: (a: { folderId: TabFolderId; name: string }) =>
      commands.renameTabFolder(workspaceId, a.folderId, a.name),
    onSuccess: invalidate,
  })
  const { openIslandMenu } = useIslandMenu()

  const discardedTabs = useDiscardedTabs()
  const wrapRef = useRef<HTMLDivElement>(null)
  // Workspace switches slide the strip in from the travel direction.
  useWorkspaceSlide('top-strip', workspaceId, workspaceOrder, wrapRef)
  const stripRef = useRef<HTMLDivElement>(null)
  const activeRef = useRef<HTMLDivElement>(null)
  const [stripWidth, setStripWidth] = useState(0)
  const [stripResizing, setStripResizing] = useState(false)
  const [renamingId, setRenamingId] = useState<TabFolderId | null>(null)
  const [closingIslandId, setClosingIslandId] = useState<TabFolderId | null>(null)
  const [openingIslandId, setOpeningIslandId] = useState<TabFolderId | null>(null)
  const [openingArmed, setOpeningArmed] = useState(false)
  const [closingIds, setClosingIds] = useState<ReadonlySet<TabId>>(new Set())
  useLayoutEffect(() => {
    if (openingIslandId === null || openingArmed) return
    const raf = requestAnimationFrame(() => setOpeningArmed(true))
    return () => cancelAnimationFrame(raf)
  }, [openingIslandId, openingArmed])
  useEffect(() => {
    if (openingIslandId === null || !openingArmed) return
    const t = window.setTimeout(() => {
      setOpeningIslandId(null)
      setOpeningArmed(false)
    }, 240)
    return () => window.clearTimeout(t)
  }, [openingIslandId, openingArmed])
  // FLIP close: snapshot every sizable tab's left edge before the close lands,
  // so the layout effect can glide each survivor from its old slot to its new
  // one once the closing tab has been pulled out of layout instantly.
  const flipFromRef = useRef<Map<string, { left: number; width: number }>>(new Map())
  const captureFlip = useCallback(() => {
    const strip = stripRef.current
    if (!strip) return
    const map = new Map<string, { left: number; width: number }>()
    strip
      .querySelectorAll<HTMLElement>('[data-tab-id], [data-mtab-id]')
      .forEach((el) => {
        const id = el.dataset.tabId ?? el.dataset.mtabId
        if (!id) return
        const r = el.getBoundingClientRect()
        map.set(id, { left: r.left, width: r.width })
      })
    flipFromRef.current = map
  }, [])
  // Dedupe a single close.
  const handledRef = useRef<Set<TabId>>(new Set())
  const markClosing = useCallback((id: TabId) => {
    setClosingIds((prev) => (prev.has(id) ? prev : new Set(prev).add(id)))
    window.setTimeout(() => {
      handledRef.current.delete(id)
      setClosingIds((prev) => {
        if (!prev.has(id)) return prev
        const next = new Set(prev)
        next.delete(id)
        return next
      })
    }, TAB_EXIT_MS + 400)
  }, [])
  const beginClose = useCallback(
    (id: TabId) => {
      if (handledRef.current.has(id)) return
      handledRef.current.add(id)
      captureFlip()
      markClosing(id)
    },
    [captureFlip, markClosing],
  )
  const closeHere = useCallback(
    (id: TabId) => {
      beginClose(id)
      const returnTo = closeReturnTarget(id)
      noteTabClosed(id)
      void commands.closeTab(id, returnTo, recentTabs()).catch(() => undefined)
    },
    [beginClose],
  )
  useEffect(() => onTabClosing(beginClose), [beginClose])
  useLayoutEffect(() => {
    const from = flipFromRef.current
    if (from.size === 0) return
    flipFromRef.current = new Map()
    const strip = stripRef.current
    if (!strip) return
    const els = Array.from(
      strip.querySelectorAll<HTMLElement>('[data-tab-id], [data-mtab-id]'),
    )
    // Prevent stacking animations.
    for (const el of els) {
      for (const a of el.getAnimations()) if (a.id === FLIP_ANIM_ID) a.cancel()
    }
    const moves: Array<{ el: HTMLElement; dx: number; sx: number }> = []
    for (const el of els) {
      const id = el.dataset.tabId ?? el.dataset.mtabId
      if (id === undefined || closingIds.has(id as TabId)) continue
      const old = from.get(id)
      if (old === undefined) continue
      const now = el.getBoundingClientRect()
      const dx = old.left - now.left
      const sx = now.width > 0.5 ? old.width / now.width : 1
      if (Math.abs(dx) > 0.5 || Math.abs(sx - 1) > 0.005) moves.push({ el, dx, sx })
    }
    if (moves.length === 0) return
    const cs = getComputedStyle(els[0])
    const durRaw = cs.getPropertyValue('--reflow-dur').trim()
    const dur = durRaw.endsWith('ms')
      ? parseFloat(durRaw)
      : durRaw.endsWith('s')
        ? parseFloat(durRaw) * 1000
        : 0
    if (!(dur > 0)) return // motion off: the snap is intended
    for (const { el, dx, sx } of moves) {
      const anim = el.animate(
        [
          { transform: `translateX(${dx}px) scaleX(${sx})`, transformOrigin: 'left center' },
          { transform: 'translateX(0px) scaleX(1)', transformOrigin: 'left center' },
        ],
        { duration: dur, easing: 'linear' },
      )
      anim.id = FLIP_ANIM_ID
    }
  }, [closingIds])
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
  const pinnedTabs = displayTabs.filter((t) => pinnedSet.has(t.id))
  const stripTabs = displayTabs.filter((t) => !pinnedSet.has(t.id))

  // Sections: folders (islands) grouped together, then loose tabs.
  const folderGroups: FolderGroup[] = [...folders]
    .sort((a, b) => a.position - b.position)
    .map((folder) => ({ folder, members: stripTabs.filter((t) => t.folder_id === folder.id) }))
    .filter((g) => g.members.length > 0)
  const looseTabs = stripTabs.filter((t) => t.folder_id == null)
  const looseIds = new Set(looseTabs.map((t) => t.id))
  const sizableTabs: Tab[] = [...looseTabs]
  let collapsedCount = 0
  for (const g of folderGroups) {
    if (g.folder.collapsed || g.folder.id === closingIslandId) collapsedCount += 1
    else sizableTabs.push(...g.members)
  }

  const {
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
  } = useTabStripDrag({
    workspaceId,
    activeTabId,
    stripRef,
    tabs,
    looseTabs,
    looseIds,
    pinnedTabs,
    pinnedSet,
    folderGroups,
  })
  useLayoutEffect(() => {
    runPendingGlide(stripRef.current, pendingGlideRef)
  })

  // Accordion bookkeeping: which expanded islands were opened least recently.
  // Session-scoped per workspace, seeded in position order (oldest first).
  const expandRecencyRef = useRef<{ ws: WorkspaceId; order: TabFolderId[] } | null>(null)
  if (expandRecencyRef.current?.ws !== workspaceId) {
    expandRecencyRef.current = {
      ws: workspaceId,
      order: folderGroups.filter((g) => !g.folder.collapsed).map((g) => g.folder.id),
    }
  }

  // Expand an island; past the limit, the least-recently-expanded expanded
  // island collapses with the usual exit sweep (extras, only possible after
  // the limit was lowered mid-session, collapse on the same frame).
  const expandIsland = (folderId: TabFolderId) => {
    if (folderReorderRef.current) {
      folderReorderRef.current = false
      return
    }
    const recOrder = expandRecencyRef.current?.order ?? []
    const expandedIds = folderGroups
      .filter((g) => !g.folder.collapsed && g.folder.id !== folderId)
      .map((g) => g.folder.id)
    // Oldest first; islands missing from the recency list count as oldest.
    const byAge = [
      ...expandedIds.filter((id) => !recOrder.includes(id)),
      ...recOrder.filter((id) => expandedIds.includes(id)),
    ]
    const over = maxExpandedFolders > 0 ? expandedIds.length + 1 - maxExpandedFolders : 0
    const victims = byAge.slice(0, Math.max(0, over))
    for (const extra of victims.slice(1)) {
      setCollapsed.mutate({ folderId: extra, collapsed: true })
    }
    if (victims.length > 0) {
      const victim = victims[0]
      setClosingIslandId(victim)
      window.setTimeout(() => {
        setCollapsed.mutate({ folderId: victim, collapsed: true })
        setClosingIslandId(null)
      }, ISLAND_EXIT_MS)
    }
    expandRecencyRef.current = {
      ws: workspaceId,
      order: [...recOrder.filter((id) => id !== folderId), folderId],
    }
    setOpeningIslandId(folderId)
    setOpeningArmed(false)
    setCollapsed.mutate({ folderId, collapsed: false })
  }

  useLayoutEffect(() => {
    const strip = stripRef.current
    if (!strip) return
    let timer: number | null = null
    let prevW = 0
    const update = () => {
      const w = strip.clientWidth
      if (prevW !== 0 && Math.abs(prevW - w) > 0.5) {
        setStripResizing(true)
        if (timer !== null) window.clearTimeout(timer)
        timer = window.setTimeout(() => setStripResizing(false), 160)
      }
      prevW = w
      setStripWidth(w)
    }
    update()
    const ro = new ResizeObserver(update)
    ro.observe(strip)
    return () => {
      ro.disconnect()
      if (timer !== null) window.clearTimeout(timer)
    }
  }, [])

  useLayoutEffect(() => {
    activeRef.current?.scrollIntoView({ block: 'nearest', inline: 'center', behavior: 'smooth' })
  }, [activeTabId])

  const minActive = compact ? COMPACT_MINIMUM_CURRENT_TAB_SIZE : MINIMUM_CURRENT_TAB_SIZE
  const minInactive = compact ? COMPACT_MINIMUM_TAB_SIZE : MINIMUM_TAB_SIZE

  if (folderGroups.length === 0 && looseTabs.length === 0 && pinnedTabs.length === 0) {
    return (
      <div className={cx('kb-tabs-h-wrap', { compact })} ref={wrapRef}>
        <div className={cx('tabs-h', { compact })} ref={stripRef} />
      </div>
    )
  }

  const hasAudioIcon = (t: Tab) => tabAudio.audible.has(t.id) || tabAudio.muted.has(t.id)
  const liveOf = <T extends { id: TabId }>(list: T[]): T[] =>
    closingIds.size === 0 ? list : list.filter((t) => !closingIds.has(t.id))
  const effectiveActiveId: TabId | null = (() => {
    if (activeTabId === null || !closingIds.has(activeTabId)) return activeTabId
    const predicted = predictActiveAfterClose(
      activeTabId,
      activeTabId,
      tabs.map((t) => t.id),
      paneTree,
      recentTabs(),
    )
    return predicted ?? activeTabId
  })()
  const { sizing, isMany, inactiveCompact, audioExtra, activeVisible } = computeStripSizing({
    sizableTabs: liveOf(sizableTabs),
    looseTabs: liveOf(looseTabs),
    pinnedCount: liveOf(pinnedTabs).length,
    folderGroups,
    collapsedCount,
    closingIslandId,
    stripWidth,
    compact,
    activeTabId: effectiveActiveId,
    minActive,
    minInactive,
    isAudio: hasAudioIcon,
  })

  const sizeFor = (tab: Tab) => {
    const base =
      tab.id === effectiveActiveId && activeVisible ? sizing.activeSize : sizing.inactiveSize
    return base + (hasAudioIcon(tab) ? audioExtra : 0)
  }
  // Expanded-island members: same share as loose tabs, but capped (see
  // MEMBER_TAB_MAX) so filing always reads as the tab tucking into the pill.
  const memberSizeFor = (tab: Tab) => {
    const isActive = tab.id === effectiveActiveId && activeVisible
    const base = Math.min(
      isActive ? sizing.activeSize : sizing.inactiveSize,
      isActive ? minActive : compact ? COMPACT_MEMBER_TAB_MAX : MEMBER_TAB_MAX,
    )
    return base + (hasAudioIcon(tab) ? audioExtra : 0)
  }

  const renderTab = (
    tab: Tab,
    opts: {
      looseIndex?: number
      memberOf?: TabFolderId
      memberIndex?: number
      memberFrom?: number
    } = {},
  ) => {
    const isActive = tab.id === effectiveActiveId
    const loose = opts.looseIndex !== undefined
    const member = opts.memberOf !== undefined
    const scope = dragScopeRef.current
    const isSource = dragPart?.id === tab.id
    const isClosing = closingIds.has(tab.id)
    // Optimistic resize.
    const lifted = isSource && scope !== (member ? opts.memberOf : loose ? 'loose' : null)
    let shift = 0
    if (dragPart && dropTarget !== null && dropTarget.index >= 0) {
      if (loose && scope === 'loose') {
        const from = looseTabs.findIndex((t) => t.id === dragPart.id)
        shift = partOffset(opts.looseIndex ?? -1, from, dropTarget.index, railSide === 'right' ? -1 : 1)
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
          flex:
            lifted || isClosing
              ? '0 0 0px'
              : `${loose && !sizing.scrolling && (dragPart === null || scope === 'loose') ? '1 1' : '0 0'} ${
                  member ? memberSizeFor(tab) : sizeFor(tab)
                }px`,
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
        }}
        onContextMenu={(e) => openSplitMenu(e, tab.id)}
        onMouseDown={(e) => {
          if (e.button === 1) e.preventDefault()
        }}
        onAuxClick={(e) => {
          if (e.button === 1) {
            e.preventDefault()
            closeHere(tab.id)
          }
        }}
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
            <TabAudioButton
              id={tab.id}
              audible={tabAudio.audible.has(tab.id)}
              muted={tabAudio.muted.has(tab.id)}
              onToggle={tabAudio.toggleMute}
            />
            <span className="ttl">{tabDisplayName(tab)}</span>
          </>
        )}
        <button
          type="button"
          className="close"
          aria-label="Close tab"
          onClick={(e) => {
            e.stopPropagation()
            closeHere(tab.id)
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
          onClick={() => expandIsland(folder.id)}
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
    const sizedMembers = (
      liftedOut ? members.filter((m) => m.id !== dragPart?.id) : members
    ).filter((m) => !closingIds.has(m.id))
    const memberSum = sizedMembers.reduce((s, m) => s + memberSizeFor(m), 0)
    const externalTarget =
      dragPart !== null &&
      memberFrom < 0 &&
      dragScopeRef.current === folder.id &&
      dropTarget !== null &&
      dropTarget.index >= 0
    const opening = openingIslandId === folder.id
    const islandW =
      closing || (opening && !openingArmed)
        ? ISLAND_CIRCLE_W
        : chipWidthFor(folder, compact) +
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
          'is-opening': opening,
          'is-dragsource': isIslandSource,
        })}
        style={{ flex: `0 0 ${islandW}px`, ...(islandStyle ?? {}) }}
      >
        <div
          className={cx('h-island-name', `accent-${folder.accent}`, {
            selected: isSelected,
            'ring-contrast': isThemeAccent,
          })}
          style={{ flex: `0 0 ${chipWidthFor(folder, compact)}px` }}
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
    // Tab and pin clicks move the target through set_active_tab and folder
    // islands set it themselves; only a click on bare strip background clears it.
    if ((e.target as Element).closest('.h-tab, .h-pin-circle, .h-island, .h-island-circle')) return
    setTarget(null)
  }

  const pinnedEls = pinnedTabs.map((tab, i) => renderPinCircle(tab, i))
  const folderEls = folderGroups.map((g, i) => renderFolder(g, i))
  const looseEls = looseTabs.map((tab, i) => renderTab(tab, { looseIndex: i }))
  // Folders and loose tabs flow together in one scrollable run.
  const runEls =
    railSide === 'right' ? [...looseEls, ...folderEls] : [...folderEls, ...looseEls]
  const stripRun =
    runEls.length > 0 ? (
      <div className="h-strip-run" key="strip-run">
        {runEls}
      </div>
    ) : null
  // Rail-side ordering: pinned sits nearest the rail.
  const ordered = railSide === 'right' ? [stripRun, ...pinnedEls] : [...pinnedEls, stripRun]

  return (
    <div
      className={cx('kb-tabs-h-wrap', {
        compact,
        'is-many': isMany,
        'is-scrolling': sizing.scrolling,
        'is-inactive-compact': inactiveCompact,
        'is-strip-resizing': stripResizing,
        'is-tab-closing': closingIds.size > 0,
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
