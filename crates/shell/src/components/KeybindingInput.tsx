/**
 * Single accelerator pill for a `KeyAction` row in the Settings page.
 *
 * Click → enters "Press shortcut…" capture mode.
 * Next valid keystroke → `onChange(accelerator)`, exits capture mode.
 * Escape (in capture mode) → cancel without firing onChange.
 * Reset button (visible when current binding differs from default) →
 *   `onChange(null)` so the parent can clear the override.
 */

import { useRef, useState } from 'react'
import type { KeyboardEvent as ReactKeyboardEvent, MouseEvent } from 'react'

import { cx } from '@/lib/cx'
import { keyboardEventToAccelerator, prettyAccelerator } from '@/lib/keybindings'

interface KeybindingInputProps {
  /** Current accelerator, e.g. `"Cmd+T"`. `null` when the action is
   *  unbound. */
  value: string | null
  /** Default accelerator for this action. The reset button only shows
   *  when `value !== defaultValue`. */
  defaultValue: string | null
  /** Called with the new accelerator on capture, or `null` when the
   *  user clicks the reset button. */
  onChange: (next: string | null) => void
  /** When `true`, the pill renders with a warning treatment to surface
   *  duplicate-accelerator conflicts. */
  conflicting?: boolean
  /** Accessible label for the row, e.g. "New tab". */
  ariaLabel?: string
}

export function KeybindingInput({
  value,
  defaultValue,
  onChange,
  conflicting,
  ariaLabel,
}: KeybindingInputProps) {
  const [recording, setRecording] = useState(false)
  const buttonRef = useRef<HTMLButtonElement | null>(null)

  const startRecord = () => {
    setRecording(true)
    requestAnimationFrame(() => buttonRef.current?.focus())
  }

  const stopRecord = () => {
    setRecording(false)
  }

  const onKeyDown = (e: ReactKeyboardEvent<HTMLButtonElement>) => {
    if (!recording) return
    if (e.key === 'Escape') {
      e.preventDefault()
      stopRecord()
      return
    }
    e.preventDefault()
    e.stopPropagation()
    const accel = keyboardEventToAccelerator(e)
    if (!accel) {
      return
    }
    onChange(accel)
    stopRecord()
  }

  const onReset = (e: MouseEvent<HTMLButtonElement>) => {
    e.stopPropagation()
    onChange(null)
  }

  const display = recording
    ? 'Press shortcut…'
    : value
      ? prettyAccelerator(value)
      : '—'

  const showReset = !recording && value !== defaultValue

  return (
    <span className="kb-keybind">
      <button
        ref={buttonRef}
        type="button"
        className={cx('kb-keybind-pill', {
          recording,
          conflicting,
          unbound: !value,
        })}
        onClick={() => (recording ? stopRecord() : startRecord())}
        onKeyDown={onKeyDown}
        onBlur={() => recording && stopRecord()}
        aria-label={ariaLabel ? `${ariaLabel} shortcut` : 'Shortcut'}
      >
        {display}
      </button>
      {showReset && (
        <button
          type="button"
          className="kb-keybind-reset"
          onClick={onReset}
          aria-label="Reset to default"
          title="Reset to default"
        >
          ↺
        </button>
      )}
    </span>
  )
}
