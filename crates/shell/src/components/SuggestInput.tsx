import { useId, useState, type KeyboardEvent } from 'react'

import { cx } from '@/lib/cx'
import { matchSuggestions, type Suggestion } from '@/lib/suggest'

interface SuggestInputProps {
  value: string
  onChange: (value: string) => void
  /** Saves the input on a pick, on Enter and when the field loses focus. */
  onCommit: (value: string) => void
  suggestions: readonly Suggestion[]
  placeholder?: string
  ariaLabel: string
}

/** A free-text field with a themed list of suggestions under it. */
export function SuggestInput({ value, onChange, onCommit, suggestions, placeholder, ariaLabel }: SuggestInputProps) {
  const listId = useId()
  const [open, setOpen] = useState(false)
  const [typed, setTyped] = useState(false)
  const [active, setActive] = useState(-1)
  const shown = open ? matchSuggestions(suggestions, value, typed) : []

  const show = () => {
    setOpen(true)
    setTyped(false)
    setActive(-1)
  }
  const pick = (picked: string) => {
    setOpen(false)
    onChange(picked)
    onCommit(picked)
  }

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      if (!open) return show()
      if (shown.length === 0) return
      const step = e.key === 'ArrowDown' ? 1 : -1
      setActive((i) => (i + step + shown.length) % shown.length)
    } else if (e.key === 'Enter') {
      e.preventDefault()
      if (active >= 0 && active < shown.length) return pick(shown[active].value)
      setOpen(false)
      onCommit(value)
    } else if (e.key === 'Escape' && shown.length > 0) {
      e.preventDefault()
      e.stopPropagation()
      setOpen(false)
    }
  }

  return (
    <div className="kb-suggest">
      <input
        type="text"
        className="kb-text-input"
        role="combobox"
        aria-label={ariaLabel}
        aria-expanded={shown.length > 0}
        aria-controls={shown.length > 0 ? listId : undefined}
        aria-autocomplete="list"
        aria-activedescendant={active >= 0 && active < shown.length ? `${listId}-${active}` : undefined}
        placeholder={placeholder}
        value={value}
        spellCheck={false}
        autoComplete="off"
        onFocus={show}
        onMouseDown={() => !open && show()}
        onChange={(e) => {
          onChange(e.currentTarget.value)
          setOpen(true)
          setTyped(true)
          setActive(-1)
        }}
        onBlur={() => {
          setOpen(false)
          onCommit(value)
        }}
        onKeyDown={onKeyDown}
      />
      {shown.length > 0 && (
        <ul id={listId} role="listbox" aria-label={ariaLabel} className="kb-suggest-list">
          {shown.map((item, i) => (
            <li
              key={item.value}
              id={`${listId}-${i}`}
              role="option"
              aria-selected={i === active}
              className={cx({ 'is-active': i === active })}
              onMouseEnter={() => setActive(i)}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => pick(item.value)}
            >
              <span>{item.value}</span>
              {item.label && item.label !== item.value && <span>{item.label}</span>}
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}
