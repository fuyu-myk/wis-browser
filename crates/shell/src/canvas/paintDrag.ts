import {
  edgeGeometry,
  frameAround,
  type Box,
  type CanvasCard,
  type CanvasEdge,
  type CanvasFrame,
} from '@/lib/canvas'

import { GROUP_PAD, type GroupOutline } from './CanvasGroups'
import { transformOf } from './dom'

export interface Dragged {
  bases: Map<string, CanvasCard>
  frames: Map<string, CanvasFrame>
  edges: CanvasEdge[]
  dx: number
  dy: number
}

export interface DragTargets {
  cards: ReadonlyMap<string, HTMLElement>
  frames: ReadonlyMap<string, HTMLElement>
  groups: ReadonlyMap<string, GroupOutline>
  edges: ReadonlyMap<string, SVGGraphicsElement[]>
}

/** Moves what a drag carries, and the outlines and arrows that follow it. */
export function paintDrag(
  d: Dragged,
  all: readonly CanvasCard[],
  els: DragTargets,
  boxOf: (card: CanvasCard, dx?: number, dy?: number) => Box,
): void {
  for (const [id, base] of d.bases) {
    const el = els.cards.get(id)
    if (el) el.style.transform = transformOf(base.x + d.dx, base.y + d.dy)
  }
  for (const [id, base] of d.frames) {
    const el = els.frames.get(id)
    if (el) el.style.transform = transformOf(base.x + d.dx, base.y + d.dy)
  }
  const cards = new Map(all.map((c) => [c.id, c]))
  const placed = (id: string) => {
    const card = cards.get(id)
    if (!card) return null
    return d.bases.has(id) ? boxOf(card, d.dx, d.dy) : boxOf(card)
  }
  for (const g of els.groups.values()) {
    if (!g.members.some((id) => d.bases.has(id))) continue
    const box = frameAround(
      g.members.flatMap((id) => placed(id) ?? []),
      GROUP_PAD,
    )
    g.el.style.transform = transformOf(box.x, box.y)
    g.el.style.width = `${box.w}px`
    g.el.style.height = `${box.h}px`
  }
  for (const edge of d.edges) {
    const a = placed(edge.from)
    const b = placed(edge.to)
    if (!a || !b) continue
    const { d: path, mid } = edgeGeometry(a, b, edge)
    for (const el of els.edges.get(edge.id) ?? []) {
      if (el instanceof SVGTextElement) {
        el.setAttribute('x', String(mid.x))
        el.setAttribute('y', String(mid.y))
      } else {
        el.setAttribute('d', path)
      }
    }
  }
}
