import { useEffect, useRef, useState, type KeyboardEvent } from 'react'

import { CARD_COLORS, type CanvasCard, type CanvasDoc } from '@/features/canvas/lib/canvas'
import { alignCards, distributeCards, tidyCards, type Axis, type Edge } from '@/features/canvas/lib/canvasArrange'
import {
  propsView,
  readNumber,
  setCards,
  setCardsColor,
  setEdge,
  setFrame,
  setFrameColor,
  setFrameTitle,
  type EdgeHeads,
  type EdgeShape,
  type Field,
  type Picked,
  type PropsView,
  type Tint,
} from '@/features/canvas/lib/canvasProps'
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
          <Colors value={shown.color} onPick={(tint) => onChange(setCardsColor(doc, shown.ids, tint))} />
          {shown.ids.length > 1 && (
            <Arrange
              count={shown.ids.length}
              align={(edge) => onChange(alignCards(doc, shown.ids, edge, heightOf))}
              distribute={(axis) => onChange(distributeCards(doc, shown.ids, axis, heightOf))}
              tidy={() => onChange(tidyCards(doc, shown.ids, heightOf))}
            />
          )}
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
          <Colors value={shown.color} onPick={(tint) => onChange(setFrameColor(doc, shown.id, tint))} />
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
          <div className="canvas-props-grid canvas-props-gap">
            <TextField
              label="Label"
              value={shown.label}
              onCommit={(label) => onChange(setEdge(doc, shown.id, { label }))}
            />
          </div>
          <Choice
            label="Line"
            options={SHAPES}
            value={shown.shape}
            onPick={(shape) => onChange(setEdge(doc, shown.id, { shape }))}
          />
          <Choice
            label="Heads"
            options={HEADS}
            value={shown.heads}
            onPick={(heads) => onChange(setEdge(doc, shown.id, { heads }))}
          />
        </>
      )}
    </aside>
  )
}

const TINTS: readonly Tint[] = ['none', ...CARD_COLORS]
const TINT_NAMES: Record<Tint, string> = {
  none: 'No colour',
  accent: 'Accent',
  red: 'Red',
  orange: 'Orange',
  yellow: 'Yellow',
  green: 'Green',
  blue: 'Blue',
  purple: 'Purple',
}

/** The palette; with mixed colours selected, no swatch is on. */
function Colors({ value, onPick }: { value: Tint | null; onPick: (tint: Tint) => void }) {
  return (
    <>
      <p className="canvas-props-label">Colour</p>
      <div className="canvas-props-colors" role="group" aria-label="Colour">
        {TINTS.map((tint) => (
          <button
            key={tint}
            type="button"
            className={cx('canvas-swatch', { 'is-on': value === tint })}
            data-color={tint}
            aria-label={TINT_NAMES[tint]}
            aria-pressed={value === tint}
            title={TINT_NAMES[tint]}
            onClick={() => onPick(tint)}
          />
        ))}
      </div>
    </>
  )
}

const ALIGNS: readonly { edge: Edge; label: string; icon: string }[] = [
  { edge: 'left', label: 'Align left edges', icon: 'M2.75 2v12M5 4h8v3H5zM5 9h5v3H5z' },
  { edge: 'centre', label: 'Align centres', icon: 'M8 2v12M3.5 4h9v3h-9zM5 9h6v3H5z' },
  { edge: 'right', label: 'Align right edges', icon: 'M13.25 2v12M3 4h8v3H3zM6 9h5v3H6z' },
  { edge: 'top', label: 'Align top edges', icon: 'M2 2.75h12M4 5h3v8H4zM9 5h3v5H9z' },
  { edge: 'middle', label: 'Align middles', icon: 'M2 8h12M4 3.5h3v9H4zM9 5h3v6H9z' },
  { edge: 'bottom', label: 'Align bottom edges', icon: 'M2 13.25h12M4 3h3v8H4zM9 6h3v5H9z' },
]
const SPREADS: readonly { axis: Axis; label: string; icon: string }[] = [
  { axis: 'across', label: 'Even the gaps across', icon: 'M2.75 2v12M13.25 2v12M6 4h4v8H6z' },
  { axis: 'down', label: 'Even the gaps down', icon: 'M2 2.75h12M2 13.25h12M4 6h8v4H4z' },
]
interface Option<T> {
  value: T
  label: string
  icon: string
}

const SHAPES: readonly Option<EdgeShape>[] = [
  { value: 'curved', label: 'Curved', icon: 'M2.5 12.5C7 12.5 9 3.5 13.5 3.5' },
  { value: 'straight', label: 'Straight', icon: 'M2.5 12.5L13.5 3.5' },
]
const HEADS: readonly Option<EdgeHeads>[] = [
  { value: 'none', label: 'No heads', icon: 'M2 8h12' },
  { value: 'end', label: 'One way', icon: 'M2 8h11.5M10 4.5L13.5 8 10 11.5' },
  { value: 'both', label: 'Both ways', icon: 'M2.5 8h11M6 4.5L2.5 8 6 11.5M10 4.5L13.5 8 10 11.5' },
]

function Choice<T extends string>({
  label,
  options,
  value,
  onPick,
}: {
  label: string
  options: readonly Option<T>[]
  value: T
  onPick: (value: T) => void
}) {
  return (
    <>
      <p className="canvas-props-label">{label}</p>
      <div className="canvas-props-choice" role="group" aria-label={label}>
        {options.map((o) => (
          <button
            key={o.value}
            type="button"
            className={cx('canvas-arrange', { 'is-on': o.value === value })}
            aria-label={o.label}
            aria-pressed={o.value === value}
            title={o.label}
            onClick={() => onPick(o.value)}
          >
            <svg viewBox="0 0 16 16" className="is-line" aria-hidden>
              <path d={o.icon} />
            </svg>
          </button>
        ))}
      </div>
    </>
  )
}

const TIDY_ICON = 'M2.5 2.5h4.5v4.5H2.5zM9 2.5h4.5v4.5H9zM2.5 9h4.5v4.5H2.5zM9 9h4.5v4.5H9z'

function ArrangeButton({ label, icon, disabled = false, onClick }: { label: string; icon: string; disabled?: boolean; onClick: () => void }) {
  return (
    <button type="button" className="canvas-arrange" aria-label={label} title={label} disabled={disabled} onClick={onClick}>
      <svg viewBox="0 0 16 16" aria-hidden>
        <path d={icon} />
      </svg>
    </button>
  )
}

/** Align, even out and tidy a selection of two or more cards. */
function Arrange({
  count,
  align,
  distribute,
  tidy,
}: {
  count: number
  align: (edge: Edge) => void
  distribute: (axis: Axis) => void
  tidy: () => void
}) {
  return (
    <>
      <p className="canvas-props-label">Arrange</p>
      <div className="canvas-props-arrange" role="group" aria-label="Arrange">
        {ALIGNS.map((a) => (
          <ArrangeButton key={a.edge} label={a.label} icon={a.icon} onClick={() => align(a.edge)} />
        ))}
        {SPREADS.map((s) => (
          <ArrangeButton
            key={s.axis}
            label={count < 3 ? `${s.label} (needs three cards)` : s.label}
            icon={s.icon}
            disabled={count < 3}
            onClick={() => distribute(s.axis)}
          />
        ))}
        <ArrangeButton label="Tidy into a grid" icon={TIDY_ICON} onClick={tidy} />
      </div>
    </>
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
