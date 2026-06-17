/**
 * Settings -> Design: chrome geometry and material (corners, tab/folder shape,
 * liquid-glass scoping/intensity, active-tab indicator, density, close-button,
 * sidebar width) plus the custom-theme builder. Every control writes through
 * the settings patch; changes apply live via `useThemeApply` (body attributes).
 *
 * Writes optimistically patch the React-Query cache so the whole `design`
 * object stays current between rapid clicks.
 */

import { useEffect, useRef, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'

import type {
  BrowserState,
  ChromeDesign,
  CloseButton,
  CornerStyle,
  Density,
  FolderStyle,
  GlassIntensity,
  GlassScopes,
  TabIndicator,
  TabShape,
  UserSettings,
  UserSettingsPatch,
} from '@/bindings/types'
import { Segmented, Switch } from '@/components/controls'
import { ThemesSection } from '@/components/ThemesSection'
import {
  SIDEBAR_WIDTH_DEFAULT,
  SIDEBAR_WIDTH_MAX,
  SIDEBAR_WIDTH_MIN,
} from '@/lib/design'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'

const CORNER_OPTIONS = [
  { value: 'round' as const, label: 'Round' },
  { value: 'soft' as const, label: 'Soft' },
  { value: 'sharp' as const, label: 'Sharp' },
] satisfies ReadonlyArray<{ value: CornerStyle; label: string }>

const TAB_SHAPE_OPTIONS = [
  { value: 'pill' as const, label: 'Pill' },
  { value: 'rounded' as const, label: 'Rounded' },
  { value: 'square' as const, label: 'Square' },
] satisfies ReadonlyArray<{ value: TabShape; label: string }>

const FOLDER_STYLE_OPTIONS = [
  { value: 'pill' as const, label: 'Pill' },
  { value: 'outline' as const, label: 'Outline' },
  { value: 'text' as const, label: 'Text' },
] satisfies ReadonlyArray<{ value: FolderStyle; label: string }>

const INTENSITY_OPTIONS = [
  { value: 'light' as const, label: 'Light' },
  { value: 'medium' as const, label: 'Medium' },
  { value: 'heavy' as const, label: 'Heavy' },
] satisfies ReadonlyArray<{ value: GlassIntensity; label: string }>

const INDICATOR_OPTIONS = [
  { value: 'bar' as const, label: 'Bar' },
  { value: 'underline' as const, label: 'Underline' },
  { value: 'filled' as const, label: 'Filled' },
  { value: 'dot' as const, label: 'Dot' },
] satisfies ReadonlyArray<{ value: TabIndicator; label: string }>

const DENSITY_OPTIONS = [
  { value: 'comfortable' as const, label: 'Comfortable' },
  { value: 'cozy' as const, label: 'Cozy' },
  { value: 'compact' as const, label: 'Compact' },
] satisfies ReadonlyArray<{ value: Density; label: string }>

const CLOSE_OPTIONS = [
  { value: 'always' as const, label: 'Always' },
  { value: 'hover' as const, label: 'On hover' },
  { value: 'hidden' as const, label: 'Hidden' },
] satisfies ReadonlyArray<{ value: CloseButton; label: string }>

interface DesignSectionProps {
  settings: UserSettings
  onUpdate: (patch: UserSettingsPatch) => void
}

export function DesignSection({ settings, onUpdate }: DesignSectionProps) {
  const qc = useQueryClient()
  const design = settings.design

  const patchSettings = (partial: Partial<UserSettings>) => {
    qc.setQueryData<BrowserState>([...BROWSER_STATE_KEY], (old) =>
      old ? { ...old, settings: { ...old.settings, ...partial } } : old,
    )
  }

  const setDesign = (partial: Partial<ChromeDesign>) => {
    const next = { ...design, ...partial }
    patchSettings({ design: next })
    onUpdate({ design: next })
  }

  const setScope = (key: keyof GlassScopes, value: boolean) =>
    setDesign({ glass_scopes: { ...design.glass_scopes, [key]: value } })

  // Density folds in the legacy `compact` flag: Compact drives the existing
  // `.compact` chrome rules, Comfortable/Cozy a `data-density` spacing scale.
  const densityValue: Density = settings.compact ? 'compact' : design.density
  const setDensity = (d: Density) => {
    const compact = d === 'compact'
    const next = { ...design, density: d }
    patchSettings({ compact, design: next })
    onUpdate({ compact, design: next })
  }

  // Sidebar width: live (optimistic) so the rail resizes during the drag, with
  // the persisted write debounced so a drag-burst is one IPC round-trip.
  const [width, setWidth] = useState(design.sidebar_width)
  useEffect(() => setWidth(design.sidebar_width), [design.sidebar_width])
  const widthTimer = useRef<number | null>(null)
  const onWidth = (v: number) => {
    setWidth(v)
    patchSettings({ design: { ...design, sidebar_width: v } })
    if (widthTimer.current !== null) window.clearTimeout(widthTimer.current)
    widthTimer.current = window.setTimeout(
      () => onUpdate({ design: { ...design, sidebar_width: v } }),
      120,
    )
  }
  const resetWidth = () => {
    if (widthTimer.current !== null) window.clearTimeout(widthTimer.current)
    setWidth(SIDEBAR_WIDTH_DEFAULT)
    setDesign({ sidebar_width: SIDEBAR_WIDTH_DEFAULT })
  }

  return (
    <section id="design" className="sec">
      <div className="sec-h">Design</div>

      <div className="row">
        <div className="l">
          <span>Corners</span>
          <span>Rounding of chrome surfaces. Sharp squares the UI; pills and circles stay round.</span>
        </div>
        <Segmented
          value={design.corners}
          onChange={(corners) => setDesign({ corners })}
          options={CORNER_OPTIONS}
          ariaLabel="Corner rounding"
        />
      </div>

      <div className="row">
        <div className="l">
          <span>Tab shape</span>
          <span>Rounded keeps each strip's native shape; Pill and Square force all tabs.</span>
        </div>
        <Segmented
          value={design.tab_shape}
          onChange={(tab_shape) => setDesign({ tab_shape })}
          options={TAB_SHAPE_OPTIONS}
          ariaLabel="Tab shape"
        />
      </div>

      <div className="row">
        <div className="l">
          <span>Folder style</span>
          <span>How a tab folder's label renders: accent pill, outline, or plain text.</span>
        </div>
        <Segmented
          value={design.folder_style}
          onChange={(folder_style) => setDesign({ folder_style })}
          options={FOLDER_STYLE_OPTIONS}
          ariaLabel="Folder style"
        />
      </div>

      <div className="row">
        <div className="l">
          <span>Active tab indicator</span>
          <span>How the active tab is marked in the sidebar.</span>
        </div>
        <Segmented
          value={design.tab_indicator}
          onChange={(tab_indicator) => setDesign({ tab_indicator })}
          options={INDICATOR_OPTIONS}
          ariaLabel="Active tab indicator"
        />
      </div>

      <div className="row">
        <div className="l">
          <span>Density</span>
          <span>Overall chrome spacing.</span>
        </div>
        <Segmented
          value={densityValue}
          onChange={setDensity}
          options={DENSITY_OPTIONS}
          ariaLabel="Density"
        />
      </div>

      <div className="row">
        <div className="l">
          <span>Tab close button</span>
          <span>When the per-tab close button is shown.</span>
        </div>
        <Segmented
          value={design.close_button}
          onChange={(close_button) => setDesign({ close_button })}
          options={CLOSE_OPTIONS}
          ariaLabel="Tab close button"
        />
      </div>

      <div className="row">
        <div className="l">
          <span>Sidebar width</span>
          <span>Applies to sidebar layouts. {width}px</span>
        </div>
        <div className="kb-design-slider">
          <input
            type="range"
            className="kb-design-range"
            min={SIDEBAR_WIDTH_MIN}
            max={SIDEBAR_WIDTH_MAX}
            value={width}
            onChange={(e) => onWidth(Number(e.target.value))}
            aria-label="Sidebar width"
          />
          <button
            type="button"
            className="kb-btn"
            onClick={resetWidth}
            disabled={width === SIDEBAR_WIDTH_DEFAULT}
          >
            Reset
          </button>
        </div>
      </div>

      <div className="sec-h">Liquid glass</div>
      <p className="kb-design-note">Takes effect when Liquid glass is on (Appearance).</p>

      <div className="row">
        <div className="l">
          <span>Glass intensity</span>
          <span>Backdrop blur strength.</span>
        </div>
        <Segmented
          value={design.glass_intensity}
          onChange={(glass_intensity) => setDesign({ glass_intensity })}
          options={INTENSITY_OPTIONS}
          ariaLabel="Glass intensity"
        />
      </div>
      <div className="row">
        <div className="l">
          <span>Glass on chrome</span>
          <span>Sidebar, top bar, and address bar.</span>
        </div>
        <Switch
          checked={design.glass_scopes.chrome}
          onChange={(v) => setScope('chrome', v)}
          label="Glass on chrome"
        />
      </div>
      <div className="row">
        <div className="l">
          <span>Glass on menus</span>
          <span>Popovers, the command palette, context menus, and dialogs.</span>
        </div>
        <Switch
          checked={design.glass_scopes.menus}
          onChange={(v) => setScope('menus', v)}
          label="Glass on menus"
        />
      </div>
      <div className="row">
        <div className="l">
          <span>Glass on tabs</span>
          <span>Tab chips, folder islands, and pinned tabs.</span>
        </div>
        <Switch
          checked={design.glass_scopes.tabs}
          onChange={(v) => setScope('tabs', v)}
          label="Glass on tabs"
        />
      </div>

      <ThemesSection />
    </section>
  )
}
