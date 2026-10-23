import { frameAround, type Box } from '@/features/canvas/lib/canvas'

import { transformOf } from '@/features/canvas/components/dom'

export const GROUP_PAD = 8

export interface GroupOutline {
  el: HTMLElement
  members: readonly string[]
}

interface CanvasGroupsProps {
  groups: ReadonlyMap<string, readonly string[]>
  boxOf: (id: string) => Box | null
  register: (group: string, outline: GroupOutline | null) => void
}

export function CanvasGroups({ groups, boxOf, register }: CanvasGroupsProps) {
  return (
    <>
      {[...groups].map(([group, ids]) => {
        const boxes = ids.flatMap((id) => boxOf(id) ?? [])
        if (boxes.length === 0) return null
        const box = frameAround(boxes, GROUP_PAD)
        return (
          <div
            key={group}
            className="canvas-group"
            ref={(el) => register(group, el && { el, members: ids })}
            style={{ width: box.w, height: box.h, transform: transformOf(box.x, box.y) }}
          />
        )
      })}
    </>
  )
}
