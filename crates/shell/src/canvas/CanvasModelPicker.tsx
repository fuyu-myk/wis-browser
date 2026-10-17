import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react'

import type { CanvasModels } from '@/lib/canvasChannel'
import { cx } from '@/lib/cx'
import { ChevronDownIcon } from '@/lib/icons'
import { modelChoices, type ModelChoice } from '@/lib/modelChoices'

interface PickerProps {
  model: string
  load: () => Promise<CanvasModels>
  choose: (model: string | null) => Promise<void>
  onError: (message: string) => void
}

export function CanvasModelPicker({ model, load, choose, onError }: PickerProps) {
  const [open, setOpen] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    const onDown = (e: PointerEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false)
    }
    window.addEventListener('pointerdown', onDown, true)
    return () => window.removeEventListener('pointerdown', onDown, true)
  }, [open])

  return (
    <div className="canvas-model" ref={rootRef}>
      <button
        type="button"
        className="canvas-model-button"
        aria-haspopup="listbox"
        aria-expanded={open}
        title="Model for the canvas agent"
        onClick={() => setOpen((v) => !v)}
      >
        <span className="canvas-model-name">{model}</span>
        <ChevronDownIcon className="canvas-model-chevron" />
      </button>
      {open && (
        <ModelList
          load={load}
          onChoose={(id) => {
            setOpen(false)
            void choose(id).catch((error: unknown) =>
              onError(error instanceof Error ? error.message : 'The model could not be changed.'),
            )
          }}
          onClose={() => setOpen(false)}
        />
      )}
    </div>
  )
}

function ModelList({
  load,
  onChoose,
  onClose,
}: {
  load: () => Promise<CanvasModels>
  onChoose: (id: string | null) => void
  onClose: () => void
}) {
  const [listing, setListing] = useState<CanvasModels | null>(null)
  const [failure, setFailure] = useState<string | null>(null)
  const [query, setQuery] = useState('')
  const [active, setActive] = useState(0)

  useEffect(() => {
    let alive = true
    load().then(
      (next) => alive && setListing(next),
      (error: unknown) => alive && setFailure(error instanceof Error ? error.message : 'The models could not be listed.'),
    )
    return () => {
      alive = false
    }
  }, [load])

  const choices = useMemo<ModelChoice[]>(() => {
    const q = query.trim().toLowerCase()
    const assistant = { id: null, label: 'Same as the assistant', hint: listing?.assistant || undefined }
    const showAssistant =
      q === '' || 'same as the assistant'.includes(q) || (listing?.assistant ?? '').toLowerCase().includes(q)
    const models = modelChoices(listing?.models ?? null, query, listing !== null || failure !== null)
    return [...(showAssistant ? [assistant] : []), ...models]
  }, [listing, failure, query])
  const current = Math.min(active, choices.length - 1)
  const selected = listing?.selected ?? null

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (choices.length === 0 && e.key !== 'Escape') return
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      const step = e.key === 'ArrowDown' ? 1 : choices.length - 1
      setActive((current + step) % choices.length)
    } else if (e.key === 'Enter') {
      e.preventDefault()
      const choice = choices[current]
      if (choice) onChoose(choice.id)
    } else if (e.key === 'Escape') {
      e.preventDefault()
      onClose()
    }
  }

  return (
    <div className="canvas-model-list">
      <input
        className="canvas-search"
        autoFocus
        placeholder="Find or type a model"
        aria-label="Find or type a model"
        value={query}
        onChange={(e) => {
          setQuery(e.currentTarget.value)
          setActive(0)
        }}
        onKeyDown={onKeyDown}
      />
      <ul role="listbox" aria-label="Models">
        {choices.map((choice, i) => (
          <li
            key={choice.id ?? '(assistant)'}
            role="option"
            aria-selected={i === current}
            className={cx('canvas-option', { 'is-active': i === current, 'is-open': choice.id === selected })}
            onPointerEnter={() => setActive(i)}
            onClick={() => onChoose(choice.id)}
          >
            <span className="canvas-option-name">{choice.label}</span>
            {choice.hint && <span className="canvas-model-hint">{choice.hint}</span>}
          </li>
        ))}
        {listing === null && failure === null && <li className="canvas-option-empty">Loading models…</li>}
        {failure !== null && <li className="canvas-option-empty">{failure}</li>}
      </ul>
    </div>
  )
}
