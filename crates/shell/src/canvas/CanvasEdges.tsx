import type { PointerEvent as ReactPointerEvent } from 'react'

import { ARROW_LENGTH, edgeGeometry, pointerArrow, type Box, type CanvasEdge } from '@/lib/canvas'
import { cx } from '@/lib/cx'

export interface Linking {
  sx: number
  sy: number
  ox: number
  oy: number
  x: number
  y: number
}

interface CanvasEdgesProps {
  edges: readonly CanvasEdge[]
  boxOf: (id: string) => Box | null
  selected: string | null
  linking: Linking | null
  onPick: (id: string, e: ReactPointerEvent<SVGPathElement>) => void
  register: (id: string, el: SVGPathElement | null) => void
}

export function CanvasEdges({ edges, boxOf, selected, linking, onPick, register }: CanvasEdgesProps) {
  return (
    <svg className="canvas-edges" width="1" height="1" aria-hidden>
      <defs>
        {(['canvas-arrow', 'canvas-arrow-on'] as const).map((id) => (
          <marker
            key={id}
            id={id}
            viewBox="0 0 10 8"
            refX="0"
            refY="4"
            markerUnits="userSpaceOnUse"
            markerWidth={ARROW_LENGTH}
            markerHeight={ARROW_LENGTH * 0.8}
            orient="auto"
          >
            <path
              d="M0,0 L10,4 L0,8 z"
              className={cx('canvas-arrowhead', { 'is-selected': id === 'canvas-arrow-on' })}
            />
          </marker>
        ))}
      </defs>
      {edges.map((edge) => {
        const a = boxOf(edge.from)
        const b = boxOf(edge.to)
        if (!a || !b) return null
        const d = edgeGeometry(a, b).d
        const on = selected === edge.id
        return (
          <g key={edge.id}>
            <path
              ref={(el) => register(edge.id, el)}
              className="canvas-edge-hit"
              data-edge={edge.id}
              d={d}
              onPointerDown={(e) => onPick(edge.id, e)}
            />
            <path
              ref={(el) => register(edge.id, el)}
              className={cx('canvas-edge', { 'is-selected': on })}
              d={d}
              markerEnd={on ? 'url(#canvas-arrow-on)' : 'url(#canvas-arrow)'}
            />
          </g>
        )
      })}
      {linking && (
        <path
          className="canvas-link"
          d={pointerArrow({ x: linking.sx, y: linking.sy }, { x: linking.ox, y: linking.oy }, linking)}
          markerEnd="url(#canvas-arrow-on)"
        />
      )}
    </svg>
  )
}
