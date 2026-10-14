import { Fragment, useEffect, useLayoutEffect, useRef } from 'react'

import { cx } from '@/lib/cx'
import { localizeAccelerator, prettyAccelerator } from '@/lib/keybindings'
import { isMacOS } from '@/lib/platform'

export interface MenuItem {
  label: string
  run: () => void
  keys?: string
  shortcut?: string
  disabled?: boolean
  danger?: boolean
  divided?: boolean
}

interface CanvasMenuProps {
  x: number
  y: number
  items: readonly MenuItem[]
  onClose: () => void
}

export interface CanvasCommands {
  newCard: (at: Point) => void
  newHtmlCard: (at: Point) => void
  newFrame: (at: Point) => void
  editCard: (id: string) => void
  group: () => void
  ungroup: () => void
  frameSelection: () => void
  remove: () => void
}

interface Point {
  x: number
  y: number
}

const EDGE_GAP = 8

const hint = (accel: string) => ({
  shortcut: prettyAccelerator(localizeAccelerator(accel)),
  keys: accel.replace('Cmd', isMacOS() ? 'Meta' : 'Control'),
})

export function canvasMenuItems(
  at: Point,
  can: { group: boolean; ungroup: boolean; frame: boolean; remove: boolean },
  commands: CanvasCommands,
  html?: string,
): MenuItem[] {
  return [
    ...(html ? [{ label: 'Edit HTML', run: () => commands.editCard(html), divided: false }] : []),
    { label: 'New card', run: () => commands.newCard(at), divided: html !== undefined },
    { label: 'New HTML card', run: () => commands.newHtmlCard(at) },
    { label: 'New frame', run: () => commands.newFrame(at) },
    { label: 'Group', ...hint('Cmd+G'), disabled: !can.group, run: commands.group, divided: true },
    { label: 'Ungroup', ...hint('Cmd+Shift+G'), disabled: !can.ungroup, run: commands.ungroup },
    { label: 'Frame selection', ...hint('Cmd+Alt+G'), disabled: !can.frame, run: commands.frameSelection },
    { label: 'Delete', ...hint('Backspace'), disabled: !can.remove, danger: true, divided: true, run: commands.remove },
  ]
}

export function CanvasMenu({ x, y, items, onClose }: CanvasMenuProps) {
  const ref = useRef<HTMLDivElement>(null)

  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const { width, height } = el.getBoundingClientRect()
    el.style.left = `${Math.max(EDGE_GAP, Math.min(x, window.innerWidth - width - EDGE_GAP))}px`
    el.style.top = `${Math.max(EDGE_GAP, Math.min(y, window.innerHeight - height - EDGE_GAP))}px`
  }, [x, y])

  useEffect(() => {
    const onDown = (e: PointerEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose()
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      e.stopPropagation()
      onClose()
    }
    window.addEventListener('pointerdown', onDown, true)
    window.addEventListener('keydown', onKey, true)
    window.addEventListener('wheel', onClose, true)
    window.addEventListener('blur', onClose)
    return () => {
      window.removeEventListener('pointerdown', onDown, true)
      window.removeEventListener('keydown', onKey, true)
      window.removeEventListener('wheel', onClose, true)
      window.removeEventListener('blur', onClose)
    }
  }, [onClose])

  return (
    <div
      ref={ref}
      className="canvas-menu canvas-context-menu"
      role="menu"
      style={{ left: x, top: y }}
      onContextMenu={(e) => e.preventDefault()}
    >
      {items.map((item) => (
        <Fragment key={item.label}>
          {item.divided && <hr />}
          <button
            type="button"
            role="menuitem"
            className={cx({ 'is-danger': item.danger })}
            disabled={item.disabled}
            aria-keyshortcuts={item.keys}
            onClick={() => {
              onClose()
              item.run()
            }}
          >
            {item.label}
            {item.shortcut && <kbd>{item.shortcut}</kbd>}
          </button>
        </Fragment>
      ))}
    </div>
  )
}
