import { useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react'

import { matchCanvases, type CanvasSummary } from '@/lib/canvas'
import { cx } from '@/lib/cx'

interface CanvasSwitcherProps {
  name: string
  currentId: string
  canvases: readonly CanvasSummary[]
  /** The list is about to open; refresh it. */
  onList: () => void
  onOpen: (id: string) => void
  onCreate: () => void
  onRename: (name: string) => void
  /** Delete the open canvas. */
  onDelete: () => void
}

type Mode =
  /** `shrinkFrom` is the rename field's width, which the pill eases down from. */
  | { kind: 'idle'; shrinkFrom?: number }
  | { kind: 'list' }
  /** `from` is the pill's width, which the field grows out of. */
  | { kind: 'rename'; from: number }
  | { kind: 'menu'; x: number; y: number; confirming: boolean }

const reducedMotion = () => window.matchMedia('(prefers-reduced-motion: reduce)').matches
/** Longest the resize transition (`canvas.css`) can take, plus a frame. */
const RESIZE_MS = 320

/**
 * The open canvas's name, top left: click to switch or create, double-click
 * or the right-click menu to rename.
 */
export function CanvasSwitcher(props: CanvasSwitcherProps) {
  const [mode, setMode] = useState<Mode>({ kind: 'idle' })
  const rootRef = useRef<HTMLDivElement>(null)
  const pillRef = useRef<HTMLButtonElement>(null)
  const idle = () => setMode({ kind: 'idle' })
  const rename = () =>
    setMode({ kind: 'rename', from: pillRef.current?.getBoundingClientRect().width ?? 0 })
  const shrinkFrom = mode.kind === 'idle' ? mode.shrinkFrom : undefined

  // Start at the field's width and ease to the pill's own after a rename.
  useLayoutEffect(() => {
    const el = pillRef.current
    if (!el || shrinkFrom === undefined || reducedMotion()) return
    const natural = el.getBoundingClientRect().width
    if (Math.abs(natural - shrinkFrom) < 1) return
    el.style.width = `${shrinkFrom}px`
    void el.offsetWidth
    el.classList.add('is-resizing')
    el.style.width = `${natural}px`
    const settle = () => {
      el.classList.remove('is-resizing')
      el.style.width = ''
    }
    el.addEventListener('transitionend', settle, { once: true })
    const fallback = window.setTimeout(settle, RESIZE_MS)
    return () => {
      window.clearTimeout(fallback)
      el.removeEventListener('transitionend', settle)
      settle()
    }
  }, [shrinkFrom])

  useEffect(() => {
    if (mode.kind !== 'list' && mode.kind !== 'menu') return
    const onDown = (e: PointerEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setMode({ kind: 'idle' })
    }
    window.addEventListener('pointerdown', onDown, true)
    return () => window.removeEventListener('pointerdown', onDown, true)
  }, [mode.kind])

  return (
    <div className="canvas-switcher" ref={rootRef}>
      {mode.kind === 'rename' ? (
        <RenameField
          name={props.name}
          from={mode.from}
          onDone={(name, width) => {
            setMode({ kind: 'idle', shrinkFrom: width })
            if (name !== null) props.onRename(name)
          }}
        />
      ) : (
        <button
          ref={pillRef}
          type="button"
          className="canvas-name"
          aria-haspopup="listbox"
          aria-expanded={mode.kind === 'list'}
          title="Switch canvas; double-click to rename"
          onClick={() => {
            if (mode.kind === 'list') {
              idle()
              return
            }
            props.onList()
            setMode({ kind: 'list' })
          }}
          onDoubleClick={rename}
          onContextMenu={(e) => {
            e.preventDefault()
            setMode({ kind: 'menu', x: e.clientX, y: e.clientY, confirming: false })
          }}
        >
          <span className="canvas-name-text">{props.name}</span>
          <svg viewBox="0 0 24 24" aria-hidden className="canvas-name-chevron">
            <path d="m6 9 6 6 6-6" />
          </svg>
        </button>
      )}

      {mode.kind === 'list' && (
        <CanvasList
          canvases={props.canvases}
          currentId={props.currentId}
          onPick={(id) => {
            idle()
            if (id !== props.currentId) props.onOpen(id)
          }}
          onCreate={() => {
            idle()
            props.onCreate()
          }}
          onClose={idle}
        />
      )}

      {mode.kind === 'menu' && (
        <div className="canvas-menu" role="menu" style={{ left: mode.x, top: mode.y }}>
          <button type="button" role="menuitem" onClick={rename}>
            Rename
          </button>
          <button
            type="button"
            role="menuitem"
            className="is-danger"
            onClick={() => {
              if (!mode.confirming) {
                setMode({ ...mode, confirming: true })
                return
              }
              idle()
              props.onDelete()
            }}
          >
            {mode.confirming ? 'Delete this canvas?' : 'Delete'}
          </button>
        </div>
      )}
    </div>
  )
}

interface CanvasListProps {
  canvases: readonly CanvasSummary[]
  currentId: string
  onPick: (id: string) => void
  onCreate: () => void
  onClose: () => void
}

/** "New canvas", then the canvases matching the search. */
function CanvasList({ canvases, currentId, onPick, onCreate, onClose }: CanvasListProps) {
  const [query, setQuery] = useState('')
  const [active, setActive] = useState(0)
  const results = useMemo(() => matchCanvases(canvases, query), [canvases, query])
  const count = results.length + 1
  const current = Math.min(active, count - 1)

  const choose = (index: number) => {
    const canvas = index > 0 ? results[index - 1] : undefined
    if (canvas) onPick(canvas.id)
    else onCreate()
  }
  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      const step = e.key === 'ArrowDown' ? 1 : count - 1
      setActive((current + step) % count)
    } else if (e.key === 'Enter') {
      e.preventDefault()
      choose(current)
    } else if (e.key === 'Escape') {
      e.preventDefault()
      onClose()
    }
  }

  return (
    <div className="canvas-list">
      <input
        className="canvas-search"
        autoFocus
        placeholder="Find a canvas"
        aria-label="Find a canvas"
        aria-controls="canvas-options"
        aria-activedescendant={`canvas-option-${current}`}
        value={query}
        onChange={(e) => {
          setQuery(e.currentTarget.value)
          setActive(e.currentTarget.value.trim() === '' ? 0 : 1)
        }}
        onKeyDown={onKeyDown}
      />
      <ul id="canvas-options" role="listbox" aria-label="Canvases">
        <li
          id="canvas-option-0"
          role="option"
          aria-selected={current === 0}
          className={cx('canvas-option', 'is-new', { 'is-active': current === 0 })}
          onPointerEnter={() => setActive(0)}
          onClick={() => choose(0)}
        >
          New canvas
        </li>
        {results.map((canvas, i) => (
          <li
            key={canvas.id}
            id={`canvas-option-${i + 1}`}
            role="option"
            aria-selected={current === i + 1}
            aria-current={canvas.id === currentId ? 'page' : undefined}
            className={cx('canvas-option', {
              'is-active': current === i + 1,
              'is-open': canvas.id === currentId,
            })}
            onPointerEnter={() => setActive(i + 1)}
            onClick={() => choose(i + 1)}
          >
            <span className="canvas-option-name">{canvas.name}</span>
          </li>
        ))}
        {results.length === 0 && query.trim() !== '' && (
          <li className="canvas-option-empty">No canvas matches</li>
        )}
      </ul>
    </div>
  )
}

interface RenameFieldProps {
  name: string
  /** The pill's width. */
  from: number
  /** The new name (null when cancelled) and the field's width as it closes. */
  onDone: (name: string | null, width: number) => void
}

/** In-place rename; resolves to the new name, or null when cancelled. */
function RenameField({ name, from, onDone }: RenameFieldProps) {
  const ref = useRef<HTMLInputElement>(null)
  const finished = useRef(false)
  const finish = (value: string | null) => {
    if (finished.current) return
    finished.current = true
    const width = ref.current?.getBoundingClientRect().width ?? 0
    onDone(value !== null && value.trim() !== '' ? value.trim() : null, width)
  }
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    if (from > 0 && !reducedMotion()) {
      el.style.width = `${from}px`
      void el.offsetWidth
      el.style.width = ''
    }
    el.classList.add('is-grown')
    el.focus()
    el.select()
  }, [from])
  return (
    <input
      ref={ref}
      className="canvas-name-input"
      defaultValue={name}
      maxLength={120}
      aria-label="Canvas name"
      onKeyDown={(e) => {
        if (e.key === 'Enter') finish(e.currentTarget.value)
        if (e.key === 'Escape') finish(null)
      }}
      onBlur={(e) => finish(e.currentTarget.value)}
    />
  )
}
