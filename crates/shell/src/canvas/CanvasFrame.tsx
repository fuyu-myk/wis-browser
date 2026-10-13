import { memo, useLayoutEffect, useRef, type PointerEvent as ReactPointerEvent } from 'react'

import type { CanvasFrame } from '@/lib/canvas'
import { cx } from '@/lib/cx'

import { transformOf } from './dom'

export interface FrameHandlers {
  register: (id: string, el: HTMLElement | null) => void
  down: (frame: CanvasFrame, e: ReactPointerEvent<HTMLElement>) => void
  resize: (frame: CanvasFrame, e: ReactPointerEvent<HTMLElement>) => void
  rename: (id: string) => void
  renamed: (id: string, title: string | null) => void
}

interface FrameProps {
  frame: CanvasFrame
  selected: boolean
  renaming: boolean
  handlers: FrameHandlers
}

export const Frame = memo(function Frame({ frame, selected, renaming, handlers }: FrameProps) {
  return (
    <div
      className={cx('canvas-frame', { 'is-selected': selected })}
      data-frame={frame.id}
      ref={(el) => handlers.register(frame.id, el)}
      style={{ width: frame.w, height: frame.h, transform: transformOf(frame.x, frame.y) }}
    >
      {renaming ? (
        <TitleField title={frame.title} onDone={(title) => handlers.renamed(frame.id, title)} />
      ) : (
        <div
          className="canvas-frame-title"
          onPointerDown={(e) => handlers.down(frame, e)}
          onDoubleClick={(e) => {
            e.stopPropagation()
            handlers.rename(frame.id)
          }}
        >
          {frame.title}
        </div>
      )}
      <span className="canvas-frame-resize" onPointerDown={(e) => handlers.resize(frame, e)} />
    </div>
  )
})

function TitleField({ title, onDone }: { title: string; onDone: (title: string | null) => void }) {
  const ref = useRef<HTMLInputElement>(null)
  const finished = useRef(false)
  const finish = (value: string | null) => {
    if (finished.current) return
    finished.current = true
    onDone(value)
  }
  useLayoutEffect(() => {
    ref.current?.focus()
    ref.current?.select()
  }, [])
  return (
    <input
      ref={ref}
      className="canvas-frame-title canvas-frame-input"
      defaultValue={title}
      maxLength={120}
      aria-label="Frame name"
      onPointerDown={(e) => e.stopPropagation()}
      onDoubleClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        if (e.key === 'Enter') finish(e.currentTarget.value)
        if (e.key === 'Escape') finish(null)
      }}
      onBlur={(e) => finish(e.currentTarget.value)}
    />
  )
}
