/**
 * Directional entrance for workspace switches: the referenced element slides
 * in from the side of the new workspace's position relative to the old one
 * (further right in the pill order = enters from the right). Previous ids
 * are remembered per scope at module level, so consumers that remount on a
 * switch (the launchpad) still know where the user came from; consumers that
 * persist (the tab strips) read the same memory.
 */

import { useLayoutEffect } from 'react'

import type { WorkspaceId } from '@/bindings/types'
import { workspaceSlideIn } from '@/lib/tabMotion'

const lastSeen = new Map<string, WorkspaceId>()

export function useWorkspaceSlide(
  scope: string,
  workspaceId: WorkspaceId | null | undefined,
  order: readonly WorkspaceId[] | undefined,
  ref: { current: HTMLElement | null },
): void {
  useLayoutEffect(() => {
    if (!workspaceId) return
    const prev = lastSeen.get(scope) ?? null
    lastSeen.set(scope, workspaceId)
    if (!order || prev === null || prev === workspaceId) return
    const from = order.indexOf(prev)
    const to = order.indexOf(workspaceId)
    if (from < 0 || to < 0 || from === to) return
    workspaceSlideIn(ref.current, to > from ? 'right' : 'left')
    // Only the workspace change should trigger; order/ref are companions.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workspaceId, scope])
}
