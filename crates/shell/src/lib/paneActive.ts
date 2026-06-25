import type { PaneNode, TabId } from '@/bindings/types'

/* Pure mirror of the parts of `wis_core` that decide which tab becomes active
   after a close, so the top strip can size + highlight the promoted tab on the
   close frame instead of waiting for the backend refetch. Kept in lockstep with
   `Workspace::with_tab_removed` + `with_pane_after_removed` and `PaneNode`. */

function contains(node: PaneNode, tab: TabId): boolean {
  return node.kind === 'leaf'
    ? node.tab === tab
    : contains(node.first, tab) || contains(node.second, tab)
}

function firstLeaf(node: PaneNode): TabId {
  return node.kind === 'leaf' ? node.tab : firstLeaf(node.first)
}

/* Mirror of `PaneNode::with_closed_leaf`: drop the leaf showing `target`,
   collapsing its parent split so the sibling takes the space. Null when the
   tree would empty. */
function withClosedLeaf(node: PaneNode, target: TabId): PaneNode | null {
  if (node.kind === 'leaf') return node.tab === target ? null : node
  const first = withClosedLeaf(node.first, target)
  const second = withClosedLeaf(node.second, target)
  if (first && second) return { ...node, first, second }
  // One side collapsed entirely: promote the survivor (null if both are gone).
  return first ?? second
}

/**
 * Predict which tab the backend will make active after closing `closing`,
 * mirroring `Workspace::with_tab_removed` then `with_pane_after_removed`.
 * `order` is the workspace tab order; `paneTree` is its split layout (or null
 * for a single-tab workspace). Returns null only if no tab would remain.
 */
export function predictActiveAfterClose(
  active: TabId,
  closing: TabId,
  order: TabId[],
  paneTree: PaneNode | null,
): TabId | null {
  // with_tab_removed: a closed active hands off to the previous tab in order,
  // or the new first when it was first.
  let next: TabId | null = active
  if (active === closing) {
    const idx = order.indexOf(closing)
    next = idx > 0 ? order[idx - 1] : (order.find((id) => id !== closing) ?? null)
  }
  // with_pane_after_removed: a split instead promotes a surviving pane leaf.
  if (!paneTree || !contains(paneTree, closing)) return next
  const pruned = withClosedLeaf(paneTree, closing)
  if (pruned === null) return next
  if (pruned.kind === 'leaf') return pruned.tab
  return next !== null && contains(pruned, next) ? next : firstLeaf(pruned)
}
