import { useRef, useState, type ReactNode } from 'react'

import type { PaneBranch, PaneNode, Tab, TabId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { isChromeUrl } from '@/lib/url'

const RATIO_SCALE = 1000
const MIN_FRACTION = 0.05
const MAX_FRACTION = 0.95

/**
 * Where a dragged tab lands on a pane: an edge (`left`/`right`/`top`/`bottom`)
 * splits the pane against that side; `center` replaces the pane's tab outright.
 */
export type PaneSide = 'left' | 'right' | 'top' | 'bottom' | 'center'

/** Half-size of the central "replace" zone, as a fraction of the pane. */
const CENTER_HALF = 0.18

interface PaneViewProps {
  node: PaneNode
  /** Path from the root tree to this node (for ratio commits). */
  path: PaneBranch[]
  tabsById: Map<TabId, Tab>
  /** The focused pane's tab id, highlighted in periwinkle. */
  focusedId: TabId | null
  onFocusPane: (tabId: TabId) => void
  onRatioCommit: (path: PaneBranch[], ratio: number) => void
  /** A tab is being dragged over the panes (pointer drag from the tab list). */
  dragActive: boolean
  /** The tab being dragged, dropped against a pane side. */
  movedTabId: TabId | null
  onPaneDrop: (targetTabId: TabId, movedTabId: TabId, side: PaneSide) => void
}

/**
 * Where in `rect` the point falls. A central box → `center` (replace); outside
 * it, the quadrant by diagonals → an edge split.
 */
function sideFromPoint(rect: DOMRect, clientX: number, clientY: number): PaneSide {
  const nx = (clientX - rect.left) / Math.max(1, rect.width)
  const ny = (clientY - rect.top) / Math.max(1, rect.height)
  if (Math.abs(nx - 0.5) <= CENTER_HALF && Math.abs(ny - 0.5) <= CENTER_HALF) return 'center'
  if (ny < nx && ny < 1 - nx) return 'top'
  if (ny > nx && ny > 1 - nx) return 'bottom'
  return nx < 0.5 ? 'left' : 'right'
}

/** Render a pane tree (or subtree) into nested flex regions. */
export function PaneView(props: PaneViewProps): ReactNode {
  const { node } = props
  if (node.kind === 'leaf') {
    return <PaneLeaf {...props} tabId={node.tab} />
  }
  return <PaneSplit {...props} node={node} />
}

function PaneLeaf({
  tabId,
  tabsById,
  focusedId,
  onFocusPane,
  dragActive,
  movedTabId,
  onPaneDrop,
}: PaneViewProps & { tabId: TabId }) {
  const tab = tabsById.get(tabId)
  const chrome = tab ? isChromeUrl(tab.url) : false
  const hintRef = useRef<HTMLDivElement | null>(null)
  const setHint = (side: PaneSide | null) => {
    const el = hintRef.current
    if (!el) return
    el.className = side ? `pane-drop-hint side-${side}` : 'pane-drop-hint is-hidden'
  }

  return (
    <div
      className={cx('pane-leaf', {
        'is-focused': tabId === focusedId,
        'is-webview': !chrome,
      })}
      onMouseDownCapture={() => onFocusPane(tabId)}
    >
      {/* The surface is the measured rect a native webview is tiled over. */}
      <div
        className="pane-surface"
        data-pane-leaf={tabId}
        {...(!chrome ? { 'data-pane-webview': 'true' } : {})}
      >
        {chrome && tab ? <ChromePane tab={tab} /> : null}
      </div>

      {/* While a tab is dragged, webviews are hidden and this overlay catches
          the pointer drag, picking a side from the cursor quadrant. */}
      {dragActive ? (
        <div
          className="pane-drop"
          onPointerMove={(e) => {
            setHint(sideFromPoint(e.currentTarget.getBoundingClientRect(), e.clientX, e.clientY))
          }}
          onPointerLeave={() => setHint(null)}
          onPointerUp={(e) => {
            const side = sideFromPoint(e.currentTarget.getBoundingClientRect(), e.clientX, e.clientY)
            setHint(null)
            if (movedTabId) onPaneDrop(tabId, movedTabId, side)
          }}
        >
          <div className="pane-drop-title">{tab ? paneLabel(tab) : ''}</div>
          <div ref={hintRef} className="pane-drop-hint is-hidden" />
        </div>
      ) : null}
    </div>
  )
}

function paneLabel(tab: Tab): string {
  return tab.title?.trim() || tab.url
}

type PaneSplitProps = PaneViewProps & { node: Extract<PaneNode, { kind: 'split' }> }

function PaneSplit({
  node,
  path,
  tabsById,
  focusedId,
  onFocusPane,
  onRatioCommit,
  dragActive,
  movedTabId,
  onPaneDrop,
}: PaneSplitProps) {
  const { dir, ratio, first, second } = node
  const row = dir === 'horizontal'
  const containerRef = useRef<HTMLDivElement | null>(null)
  const [dragRatio, setDragRatio] = useState<number | null>(null)
  const effRatio = dragRatio ?? ratio
  const firstGrow = effRatio
  const secondGrow = RATIO_SCALE - effRatio

  const onDividerDown = (e: React.PointerEvent) => {
    e.preventDefault()
    e.stopPropagation()
    const container = containerRef.current
    if (!container) return
    const rect = container.getBoundingClientRect()

    window.dispatchEvent(new CustomEvent('wis-pane-resize', { detail: true }))

    const onMove = (ev: PointerEvent) => {
      const frac = row
        ? (ev.clientX - rect.left) / rect.width
        : (ev.clientY - rect.top) / rect.height
      const clamped = Math.min(MAX_FRACTION, Math.max(MIN_FRACTION, frac))
      setDragRatio(Math.round(clamped * RATIO_SCALE))
    }
    const onUp = () => {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      window.dispatchEvent(new CustomEvent('wis-pane-resize', { detail: false }))
      setDragRatio((cur) => {
        if (cur != null) onRatioCommit(path, cur)
        return null
      })
    }
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
  }

  const childProps = {
    tabsById,
    focusedId,
    onFocusPane,
    onRatioCommit,
    dragActive,
    movedTabId,
    onPaneDrop,
  }

  return (
    <div ref={containerRef} className={cx('pane-split', row ? 'is-row' : 'is-col')}>
      <div className="pane-slot" style={{ flex: `${firstGrow} 1 0%` }}>
        <PaneView node={first} path={[...path, 'first']} {...childProps} />
      </div>
      <div
        className={cx('pane-divider', row ? 'is-vertical' : 'is-horizontal')}
        role="separator"
        aria-orientation={row ? 'vertical' : 'horizontal'}
        onPointerDown={onDividerDown}
      />
      <div className="pane-slot" style={{ flex: `${secondGrow} 1 0%` }}>
        <PaneView node={second} path={[...path, 'second']} {...childProps} />
      </div>
    </div>
  )
}

import { NewTabPage } from '@/components/NewTabPage'
import { SettingsPage } from '@/components/SettingsPage'
import { SETTINGS_URL } from '@/lib/url'

function ChromePane({ tab }: { tab: Tab }) {
  if (tab.url === SETTINGS_URL) return <SettingsPage />
  return <NewTabPage activeTab={tab} />
}
