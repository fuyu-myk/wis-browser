import { useEffect, useRef, useState, type KeyboardEvent } from 'react'

import type { CanvasCard, CanvasDoc } from '@/lib/canvas'
import {
  propsView,
  readNumber,
  setCards,
  setFrame,
  setFrameTitle,
  type Field,
  type Picked,
  type PropsView,
} from '@/lib/canvasProps'
import { cx } from '@/lib/cx'

interface PanelProps {
  doc: CanvasDoc
  selected: ReadonlySet<string>
  picked: Picked
  heightOf: (card: CanvasCard) => number
  onChange: (doc: CanvasDoc) => void
}

/** The props panel, on the side away from the sidebar: in while something is
 *  selected, out once nothing is. */
export function CanvasProps({ doc, selected, picked, heightOf, onChange }: PanelProps) {
  const view = propsView(doc, selected, picked, heightOf)
  const last = useRef<PropsView>(null)
  if (view) last.current = view
  const shown = view ?? last.current
  const panel = useRef<HTMLElement>(null)
  const open = view !== null

  useEffect(() => {
    if (panel.current) panel.current.inert = !open
  }, [open])

  const set = (field: Field, value: number) => {
    if (shown?.kind === 'cards') onChange(setCards(doc, shown.ids, field, value, heightOf))
    else if (shown?.kind === 'frame') onChange(setFrame(doc, shown.id, field, value, heightOf))
  }

  return (
    <aside ref={panel} className={cx('canvas-props', { 'is-open': open })} aria-label="Properties">
      {shown?.kind === 'cards' && (
        <>
          <Head kind={shown.label} title={shown.title} />
          <div className="canvas-props-grid">
            <NumberField label="X" value={shown.box.x} onCommit={(n) => set('x', n)} />
            <NumberField label="Y" value={shown.box.y} onCommit={(n) => set('y', n)} />
            <NumberField label="W" value={shown.w} onCommit={(n) => set('w', n)} />
            <NumberField
              label="H"
              value={shown.h}
              disabled={!shown.sized}
              hint={shown.sized ? undefined : 'Notes are as tall as their text'}
              onCommit={(n) => set('h', n)}
            />
          </div>
        </>
      )}
      {shown?.kind === 'frame' && (
        <>
          <Head kind="Frame" title="" />
          <div className="canvas-props-grid">
            <TextField
              label="Name"
              value={shown.title}
              onCommit={(title) => onChange(setFrameTitle(doc, shown.id, title))}
            />
            <NumberField label="X" value={shown.box.x} onCommit={(n) => set('x', n)} />
            <NumberField label="Y" value={shown.box.y} onCommit={(n) => set('y', n)} />
            <NumberField label="W" value={shown.box.w} onCommit={(n) => set('w', n)} />
            <NumberField label="H" value={shown.box.h} onCommit={(n) => set('h', n)} />
          </div>
        </>
      )}
      {shown?.kind === 'edge' && (
        <>
          <Head kind="Arrow" title="" />
          <dl className="canvas-props-ends">
            <dt>From</dt>
            <dd>{shown.from}</dd>
            <dt>To</dt>
            <dd>{shown.to}</dd>
          </dl>
        </>
      )}
    </aside>
  )
}

function Head({ kind, title }: { kind: string; title: string }) {
  return (
    <header className="canvas-props-head">
      <span className="canvas-props-kind">{kind}</span>
      {title !== '' && <span className="canvas-props-title">{title}</span>}
    </header>
  )
}

interface FieldProps<T> {
  label: string
  value: T
  onCommit: (value: NonNullable<T>) => void
}

/** A number typed in commits on Enter or leaving the field; arrows nudge by
 *  one (ten with Shift); Escape puts the value back. */
function NumberField({
  label,
  value,
  disabled = false,
  hint,
  onCommit,
}: FieldProps<number | null> & { disabled?: boolean; hint?: string }) {
  const [draft, setDraft] = useState<string | null>(null)
  const dropped = useRef(false)
  const shown = value === null ? '' : String(Math.round(value))
  const commit = () => {
    if (dropped.current) {
      dropped.current = false
      return
    }
    if (draft === null) return
    const typed = readNumber(draft)
    setDraft(null)
    if (typed !== null && typed !== value) onCommit(typed)
  }
  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      commit()
      e.currentTarget.select()
    } else if (e.key === 'Escape') {
      dropped.current = true
      setDraft(null)
      e.currentTarget.blur()
    } else if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
      e.preventDefault()
      const base = readNumber(draft ?? shown)
      if (base === null) return
      setDraft(null)
      onCommit(base + (e.shiftKey ? 10 : 1) * (e.key === 'ArrowUp' ? 1 : -1))
    }
  }
  return (
    <label className={cx('canvas-props-field', { 'is-disabled': disabled })} title={hint}>
      <span>{label}</span>
      <input
        inputMode="numeric"
        spellCheck={false}
        aria-label={label}
        value={draft ?? shown}
        placeholder={value === null ? 'Mixed' : undefined}
        disabled={disabled}
        onChange={(e) => setDraft(e.currentTarget.value)}
        onFocus={(e) => e.currentTarget.select()}
        onBlur={commit}
        onKeyDown={onKeyDown}
      />
    </label>
  )
}

function TextField({ label, value, onCommit }: FieldProps<string>) {
  const [draft, setDraft] = useState<string | null>(null)
  const dropped = useRef(false)
  const commit = () => {
    if (!dropped.current && draft !== null && draft !== value) onCommit(draft)
    dropped.current = false
    setDraft(null)
  }
  return (
    <label className="canvas-props-field is-wide">
      <span>{label}</span>
      <input
        spellCheck={false}
        aria-label={label}
        value={draft ?? value}
        onChange={(e) => setDraft(e.currentTarget.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') commit()
          else if (e.key === 'Escape') {
            dropped.current = true
            setDraft(null)
            e.currentTarget.blur()
          }
        }}
      />
    </label>
  )
}
