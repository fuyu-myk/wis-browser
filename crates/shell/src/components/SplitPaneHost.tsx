import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'

import type { PaneBranch, PaneNode, Tab, TabId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { isChromeUrl } from '@/lib/url'

const RATIO_SCALE = 1000
const MIN_FRACTION = 0.05
const MAX_FRACTION = 0.95

/** Duration of the chrome-side pane glide on splits (matches the CSS). */
export const PANE_GLIDE_MS = 260

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
  /** Leaves added by the last structural change: their slot glides open. */
  newLeafIds: ReadonlySet<TabId>
  /** Leaves being removed: their slot glides shut before the tree commits. */
  closingLeafIds: ReadonlySet<TabId>
  /** Tabs floating in the video popout: their pane keeps its slot and shows
   *  the black player placeholder under the (departed) webview. */
  poppedTabIds: ReadonlySet<TabId>
  /** Fold the popout back into its pane (placeholder click). */
  onRestorePopout: (tabId: TabId) => void
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
  poppedTabIds,
  onRestorePopout,
}: PaneViewProps & { tabId: TabId }) {
  const tab = tabsById.get(tabId)
  const chrome = tab ? isChromeUrl(tab.url) || tab.load_error !== null : false
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
        {/* Painted under the native webview, so it only shows once the
            webview departs into the popout window. */}
        {!chrome && poppedTabIds.has(tabId) ? (
          <button
            type="button"
            className="kb-popout-placeholder"
            onClick={() => onRestorePopout(tabId)}
            title="Bring the video back"
          >
            <svg
              width="28"
              height="28"
              viewBox="0 0 18 18"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.4"
              aria-hidden="true"
            >
              <rect x="1.5" y="3" width="15" height="12" rx="2" />
              <rect x="8.5" y="9" width="6" height="4.5" rx="1" fill="currentColor" stroke="none" />
            </svg>
            <span className="kb-popout-placeholder-label">Playing in popout</span>
            <span className="kb-popout-placeholder-hint">Click to bring the video back</span>
          </button>
        ) : null}
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

/** True when every leaf under `node` is being removed (whole side closes). */
function subtreeCloses(node: PaneNode, closing: ReadonlySet<TabId>): boolean {
  if (closing.size === 0) return false
  if (node.kind === 'leaf') return closing.has(node.tab)
  return subtreeCloses(node.first, closing) && subtreeCloses(node.second, closing)
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
  newLeafIds,
  closingLeafIds,
  poppedTabIds,
  onRestorePopout,
}: PaneSplitProps) {
  const { dir, ratio, first, second } = node
  const row = dir === 'horizontal'
  const containerRef = useRef<HTMLDivElement | null>(null)
  const [dragRatio, setDragRatio] = useState<number | null>(null)
  const effRatio = dragRatio ?? ratio

  // Entrance: a split mounted around a brand-new leaf starts that slot at
  // zero and glides it open (flex-grow transition while .is-entering).
  const [enterFrom] = useState<'first' | 'second' | null>(() => {
    if (document.body.classList.contains('motion-off')) return null
    if (first.kind === 'leaf' && newLeafIds.has(first.tab)) return 'first'
    if (second.kind === 'leaf' && newLeafIds.has(second.tab)) return 'second'
    return null
  })
  const [entered, setEntered] = useState(enterFrom === null)
  const [settled, setSettled] = useState(enterFrom === null)
  useLayoutEffect(() => {
    if (entered) return
    const raf = requestAnimationFrame(() => setEntered(true))
    return () => cancelAnimationFrame(raf)
  }, [entered])
  useEffect(() => {
    if (!entered || settled) return
    const t = window.setTimeout(() => setSettled(true), PANE_GLIDE_MS + 40)
    return () => window.clearTimeout(t)
  }, [entered, settled])

  // Exit: while TabWebViewHost keeps painting the outgoing tree, the child
  // whose subtree is being removed glides shut.
  const exitTo: 'first' | 'second' | null = subtreeCloses(first, closingLeafIds)
    ? 'first'
    : subtreeCloses(second, closingLeafIds)
      ? 'second'
      : null
  const [exitArmed, setExitArmed] = useState(false)
  useLayoutEffect(() => {
    if (exitTo === null) {
      setExitArmed(false)
      return
    }
    const raf = requestAnimationFrame(() => setExitArmed(true))
    return () => cancelAnimationFrame(raf)
  }, [exitTo])

  let firstGrow = effRatio
  let secondGrow = RATIO_SCALE - effRatio
  if (exitTo !== null && exitArmed) {
    firstGrow = exitTo === 'first' ? 0 : RATIO_SCALE
    secondGrow = exitTo === 'second' ? 0 : RATIO_SCALE
  } else if (!entered) {
    firstGrow = enterFrom === 'first' ? 0 : RATIO_SCALE
    secondGrow = enterFrom === 'second' ? 0 : RATIO_SCALE
  }

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
    newLeafIds,
    closingLeafIds,
    poppedTabIds,
    onRestorePopout,
  }

  return (
    <div
      ref={containerRef}
      className={cx('pane-split', row ? 'is-row' : 'is-col', {
        'is-entering': !settled,
        'is-exiting': exitTo !== null,
      })}
    >
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
import { HistoryPage } from '@/components/HistoryPage'
import { DownloadsPage } from '@/components/DownloadsPage'
import { NetErrorPage } from '@/components/NetErrorPage'
import { TlsErrorPage } from '@/components/TlsErrorPage'
import { DOWNLOADS_URL, HISTORY_URL, SETTINGS_URL } from '@/lib/url'

function ChromePane({ tab }: { tab: Tab }) {
  if (tab.load_error) {
    return tab.load_error.kind === 'tls_error' ? <TlsErrorPage tab={tab} /> : <NetErrorPage tab={tab} />
  }
  if (tab.url === SETTINGS_URL) return <SettingsPage tabId={tab.id} />
  if (tab.url === HISTORY_URL) return <HistoryPage activeTab={tab} />
  if (tab.url === DOWNLOADS_URL) return <DownloadsPage tabId={tab.id} />
  return <NewTabPage activeTab={tab} />
}
