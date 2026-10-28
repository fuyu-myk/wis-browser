import { useEffect, useLayoutEffect, useRef, useState, type MutableRefObject } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { BrowserState, TabFolderId, WorkspaceId } from '@/bindings/types'
import { ISLAND_EXIT_MS, type FolderGroup } from '@/features/tabs/lib/tabStripLayout'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'

interface IslandAccordionArgs {
  workspaceId: WorkspaceId
  folderGroups: FolderGroup[]
  /** At most this many islands stay expanded; 0 means no limit. */
  maxExpandedFolders: number
  /** Set by a folder reorder drag so the click that ends it is ignored. */
  folderReorderRef: MutableRefObject<boolean>
}

export interface IslandAccordion {
  /** The island playing its collapse sweep. */
  closingIslandId: TabFolderId | null
  /** The island playing its expand, armed a frame after it starts. */
  openingIslandId: TabFolderId | null
  openingArmed: boolean
  expandIsland: (folderId: TabFolderId) => void
  /** Collapse after the exit sweep has played. */
  collapseIsland: (folderId: TabFolderId) => void
}

/** Expanding and collapsing the top strip's folder islands. */
export function useIslandAccordion({
  workspaceId,
  folderGroups,
  maxExpandedFolders,
  folderReorderRef,
}: IslandAccordionArgs): IslandAccordion {
  const qc = useQueryClient()
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
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })
  const collapse = (folderId: TabFolderId, collapsed: boolean) => setCollapsed.mutate({ folderId, collapsed })

  const [closingIslandId, setClosingIslandId] = useState<TabFolderId | null>(null)
  const [openingIslandId, setOpeningIslandId] = useState<TabFolderId | null>(null)
  const [openingArmed, setOpeningArmed] = useState(false)
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

  // Which expanded islands were opened least recently. Session-scoped per
  // workspace, seeded in position order (oldest first).
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
    for (const extra of victims.slice(1)) collapse(extra, true)
    if (victims.length > 0) {
      const victim = victims[0]
      setClosingIslandId(victim)
      window.setTimeout(() => {
        collapse(victim, true)
        setClosingIslandId(null)
      }, ISLAND_EXIT_MS)
    }
    expandRecencyRef.current = {
      ws: workspaceId,
      order: [...recOrder.filter((id) => id !== folderId), folderId],
    }
    setOpeningIslandId(folderId)
    setOpeningArmed(false)
    collapse(folderId, false)
  }

  const collapseIsland = (folderId: TabFolderId) => {
    setClosingIslandId(folderId)
    window.setTimeout(() => {
      if (folderReorderRef.current) folderReorderRef.current = false
      else collapse(folderId, true)
      setClosingIslandId(null)
    }, ISLAND_EXIT_MS)
  }

  return { closingIslandId, openingIslandId, openingArmed, expandIsland, collapseIsland }
}
