/**
 * Per-workspace gradient editor, hosted on the settings page (Appearance).
 * Edits apply live (debounced through the coalesced persistence path) and
 * "Theme default" clears the override.
 */

import { useEffect, useRef, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { Background, GradientSpec, Workspace } from '@/bindings/types'
import { defaultGradient, gradientCss } from '@/lib/background'
import { PlusIcon, XIcon } from '@/lib/icons'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'
import { ColorPicker } from '@/components/controls'

const APPLY_DEBOUNCE_MS = 150
const MAX_STOPS = 8
const MAX_ANGLE = 180

interface BackgroundEditorProps {
  ws: Workspace
}

export function BackgroundEditor({ ws }: BackgroundEditorProps) {
  const qc = useQueryClient()
  const stored =
    ws.background && 'gradient' in ws.background ? ws.background.gradient : null
  const [draft, setDraft] = useState<GradientSpec | null>(stored)
  const timerRef = useRef<number | null>(null)

  // Editing follows the selected workspace.
  const wsId = ws.id
  useEffect(() => {
    setDraft(stored)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wsId])

  const apply = useMutation({
    mutationFn: (bg: Background | null) => commands.setWorkspaceBackground(ws.id, bg),
    onSuccess: () => void qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })

  // Debounce slider/picker bursts into one command.
  const update = (next: GradientSpec) => {
    setDraft(next)
    if (timerRef.current !== null) window.clearTimeout(timerRef.current)
    timerRef.current = window.setTimeout(() => {
      apply.mutate({ gradient: next })
    }, APPLY_DEBOUNCE_MS)
  }
  useEffect(
    () => () => {
      if (timerRef.current !== null) window.clearTimeout(timerRef.current)
    },
    [],
  )

  const enable = () => {
    const dark = document.body.getAttribute('data-mode') === 'dark'
    const seed = defaultGradient(dark)
    setDraft(seed)
    apply.mutate({ gradient: seed })
  }

  const reset = () => {
    setDraft(null)
    if (timerRef.current !== null) window.clearTimeout(timerRef.current)
    apply.mutate(null)
  }

  if (!draft) {
    return (
      <div className="kb-ws-bg">
        <button type="button" className="kb-ws-bg-add" onClick={enable}>
          Custom gradient
        </button>
      </div>
    )
  }

  const setAngle = (raw: number) => {
    const angle = Math.min(MAX_ANGLE, Math.max(0, Math.round(raw)))
    if (Number.isFinite(angle)) update({ ...draft, angle })
  }
  const setStop = (index: number, patch: Partial<GradientSpec['stops'][number]>) => {
    const stops = draft.stops.map((s, i) => (i === index ? { ...s, ...patch } : s))
    update({ ...draft, stops })
  }
  // Positions stay ascending: each stop is clamped between its neighbours.
  const setStopPosition = (index: number, raw: number) => {
    if (!Number.isFinite(raw)) return
    const lo = index > 0 ? draft.stops[index - 1].position : 0
    const hi = index < draft.stops.length - 1 ? draft.stops[index + 1].position : 100
    setStop(index, { position: Math.min(hi, Math.max(lo, Math.round(raw))) })
  }
  const addStop = () => {
    if (draft.stops.length >= MAX_STOPS) return
    const last = draft.stops[draft.stops.length - 1]
    update({ ...draft, stops: [...draft.stops, { ...last, position: 100 }] })
  }
  const removeStop = (index: number) => {
    if (draft.stops.length <= 2) return
    update({ ...draft, stops: draft.stops.filter((_, i) => i !== index) })
  }

  return (
    <div className="kb-ws-bg">
      <div className="kb-ws-bg-preview" style={{ background: gradientCss(draft) }} />
      <label className="kb-ws-bg-angle">
        <span>Angle</span>
        <input
          type="range"
          min={0}
          max={MAX_ANGLE}
          value={draft.angle}
          onChange={(e) => setAngle(Number(e.target.value))}
          aria-label="Gradient angle"
        />
        <input
          type="number"
          className="kb-ws-bg-angle-num"
          min={0}
          max={MAX_ANGLE}
          value={draft.angle}
          onChange={(e) => setAngle(Number(e.target.value))}
          aria-label="Gradient angle in degrees"
        />
      </label>
      <label className="kb-ws-bg-angle">
        <span>Opacity</span>
        <input
          type="range"
          min={0}
          max={100}
          value={draft.opacity}
          onChange={(e) => update({ ...draft, opacity: Number(e.target.value) })}
          aria-label="Gradient opacity"
        />
        <input
          type="number"
          className="kb-ws-bg-angle-num"
          min={0}
          max={100}
          value={draft.opacity}
          onChange={(e) => {
            const opacity = Math.min(100, Math.max(0, Math.round(Number(e.target.value))))
            if (Number.isFinite(opacity)) update({ ...draft, opacity })
          }}
          aria-label="Gradient opacity percent"
        />
      </label>
      <ul className="kb-ws-bg-stops">
        {draft.stops.map((stop, i) => (
          // Index keys are fine: stops are positional and the list is tiny.
          // eslint-disable-next-line react/no-array-index-key
          <li key={i} className="kb-ws-bg-stop">
            <ColorPicker
              value={stop.color}
              onChange={(color) => setStop(i, { color })}
              label={`Stop ${i + 1} colour`}
            />
            <input
              type="range"
              min={0}
              max={100}
              value={stop.position}
              onChange={(e) => setStopPosition(i, Number(e.target.value))}
              aria-label={`Stop ${i + 1} position`}
            />
            <input
              type="number"
              className="kb-ws-bg-angle-num"
              min={0}
              max={100}
              value={stop.position}
              onChange={(e) => setStopPosition(i, Number(e.target.value))}
              aria-label={`Stop ${i + 1} position percent`}
            />
            <button
              type="button"
              className="kb-ws-bg-del"
              aria-label="Remove stop"
              disabled={draft.stops.length <= 2}
              onClick={() => removeStop(i)}
            >
              <XIcon />
            </button>
          </li>
        ))}
      </ul>
      <div className="kb-ws-bg-actions">
        <button
          type="button"
          className="kb-ws-bg-add"
          onClick={addStop}
          disabled={draft.stops.length >= MAX_STOPS}
        >
          <PlusIcon /> Stop
        </button>
        <button type="button" className="kb-ws-bg-reset" onClick={reset}>
          Theme default
        </button>
      </div>
    </div>
  )
}
