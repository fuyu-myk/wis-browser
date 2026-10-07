import type { PaneNode, TabId } from '@/bindings/types'

/* Pure mirror of the parts of `wis_core` that decide which tab becomes active
   after a close, so the top strip can size + highlight the promoted tab on the
   close frame instead of waiting for the backend refetch. Kept in lockstep with
   the `close_tab` command's return to the previous tab,
   `BrowserState::with_tab_closed_keeping_split`, `Workspace::with_tab_removed`
   + `with_pane_after_removed` and `PaneNode`. */

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
  recent: TabId[] = [],
): TabId | null {
  if (paneTree && contains(paneTree, closing)) {
    const free = (id: TabId) => id !== closing && order.includes(id) && !contains(paneTree, id)
    const at = Math.max(0, order.indexOf(closing))
    const nearest = [...order.slice(0, at).reverse(), ...order.slice(at)].find(free)
    const refill = recent.find(free) ?? nearest
    if (refill !== undefined) return active === closing ? refill : active
  }
  const previous = recent[0]
  if (!paneTree && active === closing && previous !== undefined && previous !== closing && order.includes(previous)) {
    return previous
  }
  // with_tab_removed: a closed active hands off to the previous tab in order,
  // the new first when it was first, or the last when it was not in the order.
  let next: TabId | null = active
  if (active === closing) {
    const idx = order.indexOf(closing)
    const others = order.filter((id) => id !== closing)
    next = idx > 0 ? order[idx - 1] : idx === 0 ? (others[0] ?? null) : (others[others.length - 1] ?? null)
  }
  // with_pane_after_removed: a split instead promotes a surviving pane leaf.
  if (!paneTree || !contains(paneTree, closing)) return next
  const pruned = withClosedLeaf(paneTree, closing)
  if (pruned === null) return next
  if (pruned.kind === 'leaf') return pruned.tab
  return next !== null && contains(pruned, next) ? next : firstLeaf(pruned)
}
