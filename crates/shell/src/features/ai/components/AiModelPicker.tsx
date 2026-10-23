import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react'

import { commands } from '@/bindings/commands'
import type { AiModel, WorkspaceId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { errorMessage } from '@/lib/errors'
import { ChevronDownIcon } from '@/lib/icons'
import { modelChoices } from '@/features/ai/lib/modelChoices'

const PANE_FOCUS_EVENT = 'wis-pane-focus'

interface AiModelPickerProps {
  model: string
  workspaceId: WorkspaceId | null
  onChoose: (model: string) => Promise<void>
  onError: (message: string) => void
}

export function AiModelPicker({ model, workspaceId, onChoose, onError }: AiModelPickerProps) {
  const [open, setOpen] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    const onDown = (e: PointerEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false)
    }
    let unlisten: UnlistenFn | null = null
    let alive = true
    void listen(PANE_FOCUS_EVENT, () => setOpen(false)).then((fn) => {
      if (alive) unlisten = fn
      else fn()
    })
    window.addEventListener('pointerdown', onDown, true)
    return () => {
      alive = false
      unlisten?.()
      window.removeEventListener('pointerdown', onDown, true)
    }
  }, [open])

  return (
    <div className="kb-ai-model" ref={rootRef}>
      <button
        type="button"
        className="kb-ai-model-button"
        aria-haspopup="listbox"
        aria-expanded={open}
        title="Model for the assistant"
        onClick={() => setOpen((v) => !v)}
      >
        <span className="kb-ai-model-name">{model || 'Choose a model'}</span>
        <ChevronDownIcon className="kb-ai-model-chevron" />
      </button>
      {open && (
        <ModelList
          workspaceId={workspaceId}
          current={model}
          onChoose={(id) => {
            setOpen(false)
            if (id !== model) {
              void onChoose(id).catch((error: unknown) => onError(`The model could not be changed: ${errorMessage(error)}`))
            }
          }}
          onClose={() => setOpen(false)}
        />
      )}
    </div>
  )
}

interface ModelListProps {
  workspaceId: WorkspaceId | null
  current: string
  onChoose: (id: string) => void
  onClose: () => void
}

function ModelList({ workspaceId, current, onChoose, onClose }: ModelListProps) {
  const [models, setModels] = useState<AiModel[] | null>(null)
  const [failure, setFailure] = useState<string | null>(null)
  const [query, setQuery] = useState('')
  const [active, setActive] = useState(0)

  useEffect(() => {
    if (workspaceId === null) return
    let alive = true
    commands.aiListModels(workspaceId).then(
      (next) => alive && setModels(next),
      (error: unknown) => alive && setFailure(`The models could not be listed: ${errorMessage(error)}`),
    )
    return () => {
      alive = false
    }
  }, [workspaceId])

  const failed = workspaceId === null ? 'No workspace is open.' : failure
  const choices = useMemo(
    () => modelChoices(models, query, models !== null || failed !== null),
    [models, failed, query],
  )
  const highlighted = Math.min(active, choices.length - 1)

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Escape') {
      e.preventDefault()
      onClose()
    } else if (choices.length === 0) {
      return
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      const step = e.key === 'ArrowDown' ? 1 : choices.length - 1
      setActive((highlighted + step) % choices.length)
    } else if (e.key === 'Enter') {
      e.preventDefault()
      const id = choices[highlighted]?.id
      if (id) onChoose(id)
    }
  }

  return (
    <div className="kb-ai-model-list">
      <input
        className="kb-ai-model-search"
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
            key={choice.id}
            role="option"
            aria-selected={i === highlighted}
            className={cx('kb-ai-model-option', {
              'is-active': i === highlighted,
              'is-current': choice.id === current,
            })}
            onPointerEnter={() => setActive(i)}
            onClick={() => choice.id && onChoose(choice.id)}
          >
            <span className="kb-ai-model-option-name">{choice.label}</span>
            {choice.hint && <span className="kb-ai-model-hint">{choice.hint}</span>}
          </li>
        ))}
        {models === null && failed === null && <li className="kb-ai-model-empty">Loading models…</li>}
        {failed !== null && <li className="kb-ai-model-empty">{failed}</li>}
      </ul>
    </div>
  )
}
