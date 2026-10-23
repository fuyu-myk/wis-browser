/**
 * Per-workspace gradient editor, hosted on the settings page (Appearance).
 * Edits apply live (debounced through the coalesced persistence path) and
 * "Theme default" clears the override.
 */

import { useEffect, useRef, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type {
  Background,
  GradientSpec,
  ImageBackground,
  Workspace,
} from '@/bindings/types'
import { defaultGradient, fitCss, gradientCss } from '@/features/workspaces/lib/background'
import { PlusIcon, XIcon } from '@/lib/icons'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'
import { ColorPicker, Segmented } from '@/components/controls'

const APPLY_DEBOUNCE_MS = 150
const MAX_STOPS = 8
const MAX_ANGLE = 180
const MAX_DIM = 80
const MAX_BLUR = 40

const MODE_OPTIONS = [
  { value: 'theme' as const, label: 'Theme' },
  { value: 'gradient' as const, label: 'Gradient' },
  { value: 'image' as const, label: 'Image' },
]
type Mode = (typeof MODE_OPTIONS)[number]['value']

interface BackgroundEditorProps {
  ws: Workspace
}

export function BackgroundEditor({ ws }: BackgroundEditorProps) {
  const qc = useQueryClient()
  const stored =
    ws.background && 'gradient' in ws.background
      ? ws.background.gradient
      : (ws.background_drafts?.gradient ?? null)
  const storedImage =
    ws.background && 'image' in ws.background
      ? ws.background.image
      : (ws.background_drafts?.image ?? null)
  const [draft, setDraft] = useState<GradientSpec | null>(stored)
  const [imageDraft, setImageDraft] = useState<ImageBackground | null>(storedImage)
  // Explicit mode: the inactive variants' drafts are kept around so toggling
  // Theme -> Gradient -> Image and back restores what the user configured.
  const activeMode: Mode = !ws.background
    ? 'theme'
    : 'gradient' in ws.background
      ? 'gradient'
      : 'image'
  const [mode, setModeState] = useState<Mode>(activeMode)
  const timerRef = useRef<number | null>(null)

  // Editing follows the selected workspace.
  const wsId = ws.id
  useEffect(() => {
    setDraft(stored)
    setImageDraft(storedImage)
    setModeState(activeMode)
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

  const updateImage = (next: ImageBackground) => {
    setImageDraft(next)
    if (timerRef.current !== null) window.clearTimeout(timerRef.current)
    timerRef.current = window.setTimeout(() => {
      apply.mutate({ image: next })
    }, APPLY_DEBOUNCE_MS)
  }

  // Switch to theme default, keeping the gradient/image drafts so the
  // other modes restore the user's configuration when reselected.
  const reset = () => {
    setModeState('theme')
    if (timerRef.current !== null) window.clearTimeout(timerRef.current)
    apply.mutate(null)
  }

  const chooseImage = async () => {
    const asset = await commands.importWallpaper().catch(() => null)
    if (!asset) return
    const next: ImageBackground = {
      asset,
      fit: imageDraft?.fit ?? 'cover',
      dim: imageDraft?.dim ?? 0,
      blur: imageDraft?.blur ?? 0,
    }
    setImageDraft(next)
    setModeState('image')
    apply.mutate({ image: next })
  }

  const setMode = (next: Mode) => {
    if (next === mode) return
    if (next === 'theme') {
      reset()
    } else if (next === 'gradient') {
      const dark = document.body.getAttribute('data-mode') === 'dark'
      const seed = draft ?? stored ?? defaultGradient(dark)
      setDraft(seed)
      setModeState('gradient')
      apply.mutate({ gradient: seed })
    } else if (imageDraft) {
      setModeState('image')
      apply.mutate({ image: imageDraft })
    } else {
      // Image mode needs an asset; the current background stays until one
      // is chosen, so cancelling the picker changes nothing.
      void chooseImage()
    }
  }

  const modeSelector = (
    <Segmented
      value={mode}
      onChange={setMode}
      options={MODE_OPTIONS}
      ariaLabel="Background type"
    />
  )

  if (mode === 'image' && imageDraft) {
    return (
      <div className="kb-ws-bg">
        {modeSelector}
        <div className="kb-ws-bg-preview is-image">
          <span
            className="kb-ws-bg-preview-img"
            style={{
              backgroundImage: `url("wis-asset://wallpaper/${imageDraft.asset}")`,
              backgroundSize: fitCss(imageDraft.fit).size,
              backgroundRepeat: fitCss(imageDraft.fit).repeat,
              filter: `blur(${Math.min(4, imageDraft.blur / 6)}px)`,
            }}
            aria-hidden
          />
          <span
            className="kb-ws-bg-preview-dim"
            style={{ background: `rgba(0, 0, 0, ${(imageDraft.dim / 100).toFixed(2)})` }}
            aria-hidden
          />
        </div>
        <div className="kb-ws-bg-actions">
          <button type="button" className="kb-ws-bg-add" onClick={() => void chooseImage()}>
            Choose image
          </button>
          <Segmented
            value={imageDraft.fit}
            onChange={(fit) => updateImage({ ...imageDraft, fit })}
            options={[
              { value: 'cover' as const, label: 'Cover' },
              { value: 'contain' as const, label: 'Contain' },
              { value: 'tile' as const, label: 'Tile' },
            ]}
            ariaLabel="Image fit"
          />
        </div>
        <label className="kb-ws-bg-angle">
          <span>Dim</span>
          <input
            type="range"
            min={0}
            max={MAX_DIM}
            value={imageDraft.dim}
            onChange={(e) => updateImage({ ...imageDraft, dim: Number(e.target.value) })}
            aria-label="Image dim"
          />
          <input
            type="number"
            className="kb-ws-bg-angle-num"
            min={0}
            max={MAX_DIM}
            value={imageDraft.dim}
            onChange={(e) => {
              const dim = Math.min(MAX_DIM, Math.max(0, Math.round(Number(e.target.value))))
              if (Number.isFinite(dim)) updateImage({ ...imageDraft, dim })
            }}
            aria-label="Image dim percent"
          />
        </label>
        <label className="kb-ws-bg-angle">
          <span>Blur</span>
          <input
            type="range"
            min={0}
            max={MAX_BLUR}
            value={imageDraft.blur}
            onChange={(e) => updateImage({ ...imageDraft, blur: Number(e.target.value) })}
            aria-label="Image blur"
          />
          <input
            type="number"
            className="kb-ws-bg-angle-num"
            min={0}
            max={MAX_BLUR}
            value={imageDraft.blur}
            onChange={(e) => {
              const blur = Math.min(MAX_BLUR, Math.max(0, Math.round(Number(e.target.value))))
              if (Number.isFinite(blur)) updateImage({ ...imageDraft, blur })
            }}
            aria-label="Image blur px"
          />
        </label>
      </div>
    )
  }

  if (mode !== 'gradient' || !draft) {
    return <div className="kb-ws-bg">{modeSelector}</div>
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
      {modeSelector}
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
