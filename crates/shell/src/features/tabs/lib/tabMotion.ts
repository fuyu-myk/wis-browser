/**
 * Drag-reorder choreography shared by the tab strips: live parting offsets
 * while a drag is in flight, optimistic cache patches so a drop re-renders
 * on the same frame, and the glide-and-settle landing that carries the
 * dropped element from its ghost position into its new slot.
 */

import type { QueryClient } from '@tanstack/react-query'

import type { BrowserState, TabFolderId, TabId, WorkspaceId } from '@/bindings/types'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'
import { motionOff, motionStill } from '@/lib/motion'

/**
 * Slots a sibling shifts by while the element at `from` is dragged over
 * insertion index `to` (both within the same rendered list, dragged element
 * included): -1, 0 or +1, multiplied by the dragged slot size by the caller.
 */
export function partOffset(
  index: number,
  from: number,
  to: number,
  externalDir: 1 | -1 = 1,
): number {
  if (to < 0 || index === from) return 0
  // External drag entering the list (`from` -1): one side parts to make
  // room. `externalDir` picks which: +1 shifts everything at or after the
  // slot forward, -1 shifts everything before it backward, so a strip can
  // part toward its folder seam and keep its corner-anchored end planted.
  if (from < 0) {
    return externalDir > 0 ? (index >= to ? 1 : 0) : index < to ? -1 : 0
  }
  if (from < to && index > from && index < to) return -1
  if (from > to && index >= to && index < from) return 1
  return 0
}

/**
 * Animate `el` from the floating ghost's final rect into its committed
 * layout slot: a glide with a small settle pop on Full motion, nothing on
 * Reduced or Off.
 */
export function glideIntoPlace(el: HTMLElement, from: DOMRect): void {
  if (motionStill()) return
  const to = el.getBoundingClientRect()
  const dx = from.left - to.left
  const dy = from.top - to.top
  if (Math.abs(dx) < 1 && Math.abs(dy) < 1) return
  const move = `translate(${dx}px, ${dy}px)`
  const frames = [
    { transform: `${move} scale(1)` },
    { transform: 'translate(0, 0) scale(1.04)', offset: 0.75 },
    { transform: 'translate(0, 0) scale(1)' },
  ]
  el.animate(frames, { duration: 280, easing: 'cubic-bezier(0.2, 0.7, 0.3, 1)' })
}

/** Optimistically reorder a workspace's `tab_order` in the state cache. */
export function patchTabOrder(qc: QueryClient, workspaceId: WorkspaceId, order: TabId[]): void {
  qc.setQueryData<BrowserState>([...BROWSER_STATE_KEY], (old) =>
    old
      ? {
          ...old,
          workspaces: old.workspaces.map((ws) =>
            ws.id === workspaceId ? { ...ws, tab_order: order } : ws,
          ),
        }
      : old,
  )
}

/** Optimistically re-position a workspace's folders in the state cache. */
export function patchFolderOrder(
  qc: QueryClient,
  workspaceId: WorkspaceId,
  order: TabFolderId[],
): void {
  qc.setQueryData<BrowserState>([...BROWSER_STATE_KEY], (old) =>
    old
      ? {
          ...old,
          workspaces: old.workspaces.map((ws) =>
            ws.id === workspaceId
              ? {
                  ...ws,
                  tab_folders: ws.tab_folders.map((f) => {
                    const at = order.indexOf(f.id)
                    return at >= 0 ? { ...f, position: at } : f
                  }),
                }
              : ws,
          ),
        }
      : old,
  )
}

/** Optimistically move a tab between folders in the state cache. */
export function patchTabFolder(
  qc: QueryClient,
  tabId: TabId,
  folderId: TabFolderId | null,
): void {
  qc.setQueryData<BrowserState>([...BROWSER_STATE_KEY], (old) =>
    old
      ? {
          ...old,
          tabs: old.tabs.map((t) => (t.id === tabId ? { ...t, folder_id: folderId } : t)),
        }
      : old,
  )
}

/**
 * Directional content slide for a workspace switch ('right' = the new
 * workspace sits to the right, content enters from the right). Imperative
 * WAAPI, so no remount or layout changes are needed.
 */
export function workspaceSlideIn(el: HTMLElement | null, dir: 'left' | 'right'): void {
  if (!el || motionOff()) return
  const dx = motionStill() ? 0 : dir === 'right' ? 44 : -44
  el.animate(
    [
      { transform: `translateX(${dx}px)`, opacity: 0 },
      { transform: 'translateX(0)', opacity: 1 },
    ],
    { duration: 240, easing: 'cubic-bezier(0.2, 0.7, 0.3, 1)' },
  )
}

/** A drop landing waiting for the post-commit layout to glide into. */
export interface PendingGlide {
  selector: string
  rect: DOMRect
}

/** Run a pending glide against the post-render DOM, then clear it. */
export function runPendingGlide(
  root: HTMLElement | null,
  pending: { current: PendingGlide | null },
): void {
  const p = pending.current
  if (!p) return
  pending.current = null
  const el = root?.querySelector<HTMLElement>(p.selector)
  if (el) glideIntoPlace(el, p.rect)
}
