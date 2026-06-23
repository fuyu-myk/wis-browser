import { useState } from 'react'

import { commands } from '@/bindings/commands'
import type { KeyAction } from '@/bindings/types'
import { DEFAULT_KEYMAP, actionLabel, allActions, effectiveKeymap } from '@/lib/palette'
import { KeybindingInput } from '@/components/KeybindingInput'

interface KeyboardShortcutsSectionProps {
  keybindings: Partial<Record<KeyAction, string>>
  onChange: (patch: Partial<Record<KeyAction, string>>) => void
}

export function KeyboardShortcutsSection({
  keybindings,
  onChange,
}: KeyboardShortcutsSectionProps) {
  const [applyState, setApplyState] = useState<'idle' | 'applying' | 'applied'>('idle')

  const apply = async () => {
    setApplyState('applying')
    try {
      await commands.applyShortcuts()
      setApplyState('applied')
      setTimeout(() => setApplyState('idle'), 1500)
    } catch (err) {
      // eslint-disable-next-line no-console
      console.error('applyShortcuts failed', err)
      setApplyState('idle')
    }
  }

  const effective = effectiveKeymap(keybindings)
  const accelToActions = new Map<string, KeyAction[]>()
  for (const action of allActions()) {
    const accel = effective[action]
    if (!accel) continue
    const list = accelToActions.get(accel) ?? []
    list.push(action)
    accelToActions.set(accel, list)
  }

  return (
    <section id="shortcuts" className="sec">
      <div className="sec-h">Keyboard shortcuts</div>
      <div className="row">
        <div className="l">
          <span>Apply shortcuts</span>
          <span>
            Edits apply as you make them; click to re-apply if a new shortcut
            isn&apos;t firing yet.
          </span>
        </div>
        <button
          type="button"
          className="kb-btn"
          onClick={apply}
          disabled={applyState === 'applying'}
        >
          {applyState === 'applied' ? 'Applied' : 'Apply shortcuts'}
        </button>
      </div>
      <div className="kb-keybind-list">
        {allActions().map((action) => {
          const current = effective[action] ?? null
          const fallback = DEFAULT_KEYMAP[action] ?? null
          const conflictGroup = current ? accelToActions.get(current) ?? [] : []
          const conflicts = conflictGroup.filter((a) => a !== action)
          const isConflicting = conflicts.length > 0
          return (
            <div className="kb-keybind-row" key={action}>
              <div className="kb-keybind-row-main">
                <span className="kb-keybind-row-label">{actionLabel(action)}</span>
                {isConflicting && (
                  <span className="kb-keybind-row-conflict">
                    Also bound to{' '}
                    {conflicts.map(actionLabel).join(', ')}
                  </span>
                )}
              </div>
              <KeybindingInput
                value={current}
                defaultValue={fallback}
                conflicting={isConflicting}
                ariaLabel={actionLabel(action)}
                onChange={(next) => {
                  // `null` from the input means "reset to default".
                  onChange({ [action]: next ?? '' })
                }}
              />
            </div>
          )
        })}
      </div>
    </section>
  )
}
