/**
 * Tab context (right-click) menu, hosted by the `overlay-tabmenu` webview so it
 * composites above the tab webviews. Opened at the cursor via
 * `setOverlayBounds('tabmenu', ...)` + the `wis-tabmenu-open` event (which
 * carries the cursor position and the window height so the menu can re-fit after
 * measuring its content).
 *
 * Rows: a directional split pill + pin toggle; "Move to New Window"; a "Move to
 * workspace" section (other persistent workspaces); then "move to folder"
 * actions (each folder, remove-from-folder, and new-folder).
 */

import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { AccentColor, SplitDir, TabFolderId, TabId, WorkspaceId } from '@/bindings/types'
import { nextAccent } from '@/lib/accents'
import { cx } from '@/lib/cx'
import { PinIcon, PinOffIcon } from '@/lib/icons'
import { NEW_TAB_URL } from '@/lib/url'
import { BROWSER_STATE_KEY, useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'
import { menusGlass } from '@/lib/glass'

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

export function TabContextMenuSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)
  const qc = useQueryClient()

  const activeWorkspace = state?.workspaces.find((w) => w.id === state.active_workspace_id)
  const activeTabIdRef = useRef<TabId | null>(null)
  activeTabIdRef.current = activeWorkspace?.active_tab_id ?? null
  const workspaceIdRef = useRef(activeWorkspace?.id ?? null)
  workspaceIdRef.current = activeWorkspace?.id ?? null

  const [target, setTarget] = useState<OpenPayload | null>(null)
  // Bumped per open so the card remounts (and replays its enter animation)
  // even when the menu was already showing for another tab.
  const [openNonce, setOpenNonce] = useState(0)
  const menuRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<OpenPayload>(OPEN_EVENT, (e) => {
      setTarget(e.payload)
      setOpenNonce((n) => n + 1)
      void qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY })
      void commands.focusSelf().catch(() => undefined)
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

  // "Move to workspace" targets: other persistent workspaces. Hidden for a tab
  // in a private workspace.
  const otherWorkspaces =
    activeWorkspace && !activeWorkspace.ephemeral
      ? (state?.workspaces ?? []).filter((w) => w.id !== activeWorkspace.id && !w.ephemeral)
      : []

  // Fit the overlay to the measured menu, re-clamping vertically.
  useLayoutEffect(() => {
    if (!target) return
    const el = menuRef.current
    if (!el) return
    const h = el.offsetHeight
    const y = Math.max(MARGIN, Math.min(target.y, target.winH - h - MARGIN))
    void commands.setOverlayBounds('tabmenu', target.x, y, MENU_W, h).catch(() => undefined)
  }, [target, folders.length, inFolder, otherWorkspaces.length])

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
  const moveToNewWindow = () => {
    // Detach into a fresh workspace + window; seed it with the next free accent
    // and the tab's title (the backend handles private tabs / session carry-over).
    const accent = nextAccent((state?.workspaces ?? []).map((w) => w.accent))
    const name = movedTab?.title?.trim() || 'New Space'
    void commands.moveTabToNewWindow(target.movedTabId, name, accent).catch(() => undefined)
    close()
  }
  const moveTo = (folderId: TabFolderId | null) => {
    void commands.assignTabToFolder(target.movedTabId, folderId).catch(() => undefined)
    close()
  }
  const moveToWorkspace = (wsId: WorkspaceId) => {
    void commands.moveTabToWorkspace(target.movedTabId, wsId).catch(() => undefined)
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

  const glassMenus = menusGlass(state?.settings)

  return (
    <div
      ref={menuRef}
      key={openNonce}
      className={cx('kb-tabctx', { 'glass-menus': glassMenus })}
      role="menu"
    >
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

      <button type="button" className="kb-tabctx-item" onClick={moveToNewWindow}>
        <span className="kb-tabctx-ico" aria-hidden>
          <svg
            width="15"
            height="15"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M10 4H6a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-4" />
            <path d="M14 4h6v6" />
            <path d="M20 4 12 12" />
          </svg>
        </span>
        <span className="kb-tabctx-label">Move to New Window</span>
      </button>

      {otherWorkspaces.length > 0 && (
        <>
          <div className="kb-tabctx-sep" aria-hidden />
          <div className="kb-tabctx-section">Move to workspace</div>
          <div className="kb-tabctx-folders">
            {otherWorkspaces.map((w) => (
              <button
                key={w.id}
                type="button"
                className="kb-tabctx-item"
                onClick={() => moveToWorkspace(w.id)}
              >
                <span
                  className={cx('kb-tabctx-dot', `accent-${w.accent}`)}
                  style={w.custom_accent ? { background: w.custom_accent } : undefined}
                  aria-hidden
                />
                <span className="kb-tabctx-label">{w.name}</span>
              </button>
            ))}
          </div>
        </>
      )}

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
