/**
 * Tab "split with this tab" popup, hosted by the `overlay-tabmenu` webview so
 * it composites above the tab webviews. The chrome positions this webview at the cursor via
 * `setOverlayBounds('tabmenu', ...)` and tells it which tab was right-clicked
 * with the `wis-tabmenu-open` event.
 *
 * A 4-glyph directional pill: left/right split horizontally, up/down split
 * vertically. The arrow points where the right-clicked tab goes.
 * 
 * Two cases:
 *   - another tab → it moves into the new pane beside the active tab;
 *   - the active tab itself → it slides to the arrow side and a fresh new tab
 *     fills the opposite pane (which can then be replaced by a centre-drop).
 */

import { useEffect, useRef, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { SplitDir, TabId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { NEW_TAB_URL } from '@/lib/url'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'

const OPEN_EVENT = 'wis-tabmenu-open'
const READY_EVENT = 'wis-tabmenu-ready'
const DONE_EVENT = 'wis-tabmenu-done'

interface OpenPayload {
  movedTabId: TabId
}

type Dir = 'left' | 'up' | 'right' | 'down'

const DIRECTIONS: ReadonlyArray<{ dir: Dir; split: SplitDir; first: boolean; label: string }> = [
  { dir: 'left', split: 'horizontal', first: true, label: 'Split left' },
  { dir: 'up', split: 'vertical', first: true, label: 'Split up' },
  { dir: 'down', split: 'vertical', first: false, label: 'Split down' },
  { dir: 'right', split: 'horizontal', first: false, label: 'Split right' },
]

export function PaneSplitMenuSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)

  const activeWorkspace = state?.workspaces.find((w) => w.id === state.active_workspace_id)
  const activeTabId = activeWorkspace?.active_tab_id ?? null
  const activeTabIdRef = useRef<TabId | null>(null)
  activeTabIdRef.current = activeTabId
  const workspaceIdRef = useRef(activeWorkspace?.id ?? null)
  workspaceIdRef.current = activeWorkspace?.id ?? null

  const [movedTabId, setMovedTabId] = useState<TabId | null>(null)

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<OpenPayload>(OPEN_EVENT, (e) => {
      setMovedTabId(e.payload.movedTabId)
      void commands.focusWebview('overlay-tabmenu').catch(() => undefined)
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    void emit(READY_EVENT)
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  const close = () => {
    setMovedTabId(null)
    void commands.hideOverlay('tabmenu').catch(() => undefined)
    void emit(DONE_EVENT)
  }

  const doSplit = (split: SplitDir, first: boolean) => {
    const ws = workspaceIdRef.current
    const target = activeTabIdRef.current
    if (ws && movedTabId) {
      if (movedTabId === target) {
        void commands.splitPane(ws, split, !first, NEW_TAB_URL).catch(() => undefined)
      } else if (target) {
        void commands.splitPaneWith(ws, target, split, first, movedTabId).catch(() => undefined)
      }
    }
    close()
  }

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') close()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  const glassOn = state?.settings.glass_enabled ?? true

  return (
    <div className={cx('kb-panemenu', { 'glass-on': glassOn })} role="menu" aria-label="Split with tab">
      {DIRECTIONS.map(({ dir, split, first, label }) => (
        <button
          key={dir}
          type="button"
          role="menuitem"
          className="kb-panemenu-btn"
          aria-label={label}
          title={label}
          onClick={() => doSplit(split, first)}
        >
          <DirArrow dir={dir} />
        </button>
      ))}
    </div>
  )
}

function DirArrow({ dir }: { dir: Dir }) {
  const rotate = { right: 0, down: 90, left: 180, up: -90 }[dir]
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      style={{ transform: `rotate(${rotate}deg)` }}
      aria-hidden
    >
      <path d="M5 12h14" />
      <path d="m13 6 6 6-6 6" />
    </svg>
  )
}
