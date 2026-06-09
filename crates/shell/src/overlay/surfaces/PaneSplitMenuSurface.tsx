/**
 * Tab context menu, hosted by the `overlay-tabmenu` webview so it composites
 * above the tab webviews. Opened at the cursor via `setOverlayBounds('tabmenu',
 * ...)` + the `wis-tabmenu-open` event (which carries the cursor position and
 * the window height so the menu can re-fit after measuring its content).
 *
 * Rows: a directional split pill + pin toggle, then "move to folder" actions
 * (each folder, remove-from-folder, and new-folder).
 */

import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { AccentColor, SplitDir, TabFolderId, TabId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { PinIcon, PinOffIcon } from '@/lib/icons'
import { NEW_TAB_URL } from '@/lib/url'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'

const OPEN_EVENT = 'wis-tabmenu-open'
const READY_EVENT = 'wis-tabmenu-ready'
const DONE_EVENT = 'wis-tabmenu-done'
const MENU_W = 200
const MARGIN = 8

const ACCENTS: AccentColor[] = ['primary', 'mint', 'rose', 'sun', 'lila', 'gray', 'ink', 'azur']

interface OpenPayload {
  movedTabId: TabId
  x: number
  y: number
  winH: number
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
  const activeTabIdRef = useRef<TabId | null>(null)
  activeTabIdRef.current = activeWorkspace?.active_tab_id ?? null
  const workspaceIdRef = useRef(activeWorkspace?.id ?? null)
  workspaceIdRef.current = activeWorkspace?.id ?? null

  const [target, setTarget] = useState<OpenPayload | null>(null)
  const menuRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<OpenPayload>(OPEN_EVENT, (e) => {
      setTarget(e.payload)
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

  const movedTab = state?.tabs.find((t) => t.id === target?.movedTabId)
  const folders = [...(activeWorkspace?.tab_folders ?? [])].sort((a, b) => a.position - b.position)
  const inFolder = movedTab?.folder_id != null

  // Fit the overlay to the measured menu, re-clamping vertically.
  useLayoutEffect(() => {
    if (!target) return
    const el = menuRef.current
    if (!el) return
    const h = el.offsetHeight
    const y = Math.max(MARGIN, Math.min(target.y, target.winH - h - MARGIN))
    void commands.setOverlayBounds('tabmenu', target.x, y, MENU_W, h).catch(() => undefined)
  }, [target, folders.length, inFolder])

  const close = () => {
    setTarget(null)
    void commands.hideOverlay('tabmenu').catch(() => undefined)
    void emit(DONE_EVENT)
  }

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') close()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  if (!target) return null

  const doSplit = (split: SplitDir, first: boolean) => {
    const ws = workspaceIdRef.current
    const active = activeTabIdRef.current
    if (ws) {
      if (target.movedTabId === active) {
        void commands.splitPane(ws, split, !first, NEW_TAB_URL).catch(() => undefined)
      } else if (active) {
        void commands.splitPaneWith(ws, active, split, first, target.movedTabId).catch(() => undefined)
      }
    }
    close()
  }

  const movedPinned = !!movedTab?.pinned
  const togglePin = () => {
    void commands.pinTab(target.movedTabId, !movedPinned).catch(() => undefined)
    close()
  }
  const moveTo = (folderId: TabFolderId | null) => {
    void commands.assignTabToFolder(target.movedTabId, folderId).catch(() => undefined)
    close()
  }
  const createAndMove = async () => {
    const ws = workspaceIdRef.current
    if (!ws) return close()
    try {
      const folder = await commands.createTabFolder(ws, 'New folder', ACCENTS[folders.length % 8])
      await commands.assignTabToFolder(target.movedTabId, folder.id)
    } catch {
      // ignore
    }
    close()
  }

  const glassOn = state?.settings.glass_enabled ?? true

  return (
    <div ref={menuRef} className={cx('kb-tabctx', { 'glass-on': glassOn })} role="menu">
      <div className="kb-tabctx-split">
        {DIRECTIONS.map(({ dir, split, first, label }) => (
          <button
            key={dir}
            type="button"
            className="kb-panemenu-btn"
            aria-label={label}
            title={label}
            onClick={() => doSplit(split, first)}
          >
            <DirArrow dir={dir} />
          </button>
        ))}
        <span className="kb-panemenu-sep" aria-hidden />
        <button
          type="button"
          className="kb-panemenu-btn"
          aria-label={movedPinned ? 'Unpin tab' : 'Pin tab'}
          title={movedPinned ? 'Unpin tab' : 'Pin tab'}
          onClick={togglePin}
        >
          {movedPinned ? <PinOffIcon width={16} height={16} /> : <PinIcon width={16} height={16} />}
        </button>
      </div>

      <div className="kb-tabctx-sep" aria-hidden />

      <div className="kb-tabctx-folders">
        {folders.map((folder) => (
          <button
            key={folder.id}
            type="button"
            className="kb-tabctx-item"
            disabled={movedTab?.folder_id === folder.id}
            onClick={() => moveTo(folder.id)}
          >
            <span className={cx('kb-tabctx-dot', `accent-${folder.accent}`)} aria-hidden />
            <span className="kb-tabctx-label">{folder.name}</span>
          </button>
        ))}
        {inFolder && (
          <button type="button" className="kb-tabctx-item" onClick={() => moveTo(null)}>
            <span className="kb-tabctx-dot is-none" aria-hidden />
            <span className="kb-tabctx-label">Remove from folder</span>
          </button>
        )}
        <button type="button" className="kb-tabctx-item" onClick={createAndMove}>
          <span className="kb-tabctx-plus" aria-hidden>
            +
          </span>
          <span className="kb-tabctx-label">New folder</span>
        </button>
      </div>
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
