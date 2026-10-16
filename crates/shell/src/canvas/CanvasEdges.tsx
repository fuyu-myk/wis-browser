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
  onPick: (id: string, e: ReactPointerEvent<SVGElement>) => void
  register: (id: string, el: SVGPathElement | SVGTextElement | null) => void
}

/** Heads at an arrow's end point forward; heads at its start point back. */
const MARKERS = [
  { id: 'canvas-arrow', on: false, start: false },
  { id: 'canvas-arrow-on', on: true, start: false },
  { id: 'canvas-arrow-start', on: false, start: true },
  { id: 'canvas-arrow-start-on', on: true, start: true },
] as const

export function CanvasEdges({ edges, boxOf, selected, linking, onPick, register }: CanvasEdgesProps) {
  return (
    <svg className="canvas-edges" width="1" height="1" aria-hidden>
      <defs>
        {MARKERS.map(({ id, on, start }) => (
          <marker
            key={id}
            id={id}
            viewBox="0 0 10 8"
            refX={start ? 10 : 0}
            refY="4"
            markerUnits="userSpaceOnUse"
            markerWidth={ARROW_LENGTH}
            markerHeight={ARROW_LENGTH * 0.8}
            orient="auto"
          >
            <path d={start ? 'M10,0 L0,4 L10,8 z' : 'M0,0 L10,4 L0,8 z'} className={cx('canvas-arrowhead', { 'is-selected': on })} />
          </marker>
        ))}
      </defs>
      {edges.map((edge) => {
        const a = boxOf(edge.from)
        const b = boxOf(edge.to)
        if (!a || !b) return null
        const { d, mid } = edgeGeometry(a, b, edge)
        const on = selected === edge.id
        const lit = on ? '-on' : ''
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
              markerEnd={edge.heads === 'none' ? undefined : `url(#canvas-arrow${lit})`}
              markerStart={edge.heads === 'both' ? `url(#canvas-arrow-start${lit})` : undefined}
            />
            {edge.label && (
              <text
                ref={(el) => register(edge.id, el)}
                className={cx('canvas-edge-label', { 'is-selected': on })}
                x={mid.x}
                y={mid.y}
                onPointerDown={(e) => onPick(edge.id, e)}
              >
                {edge.label}
              </text>
            )}
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
