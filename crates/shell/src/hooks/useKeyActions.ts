/**
 * Bridge from Tauri native-menu key events to browser actions.
 *
 * The Rust side emits a `wis-keyaction` event with `{ action }` every time
 * the user hits a menu accelerator (or clicks the menu item). This hook
 * listens once at the top of the React tree and dispatches:
 *
 * - Tab + workspace ops via existing `commands`.
 * - URL-bar focus and find-in-page via window events so the matching
 *   components can respond without needing a top-down ref.
 *
 * Mount exactly once (see `Window.tsx`). Mounting multiple times would
 * fire each action N times.
 */

import { useEffect, useRef } from 'react'
import { listen } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import { closeTabAnimated } from '@/lib/tabClose'
import { DOWNLOADS_OPEN_EVENT } from '@/lib/downloads'
import { BOOKMARK_PAGE_EVENT, BOOKMARKS_OPEN_EVENT } from '@/lib/bookmarkMenu'
import type { AccentColor, BrowserState, KeyAction, PaneNode, Tab, TabId, Workspace, WorkspaceId } from '@/bindings/types'
import { HISTORY_URL, NEW_TAB_URL, SETTINGS_URL } from '@/lib/url'
import { BROWSER_STATE_KEY, useBrowserState } from '@/hooks/useBrowserState'
import { usePendingRename } from '@/hooks/usePendingRename'
import { useQueryClient } from '@tanstack/react-query'

const KEYACTION_EVENT = 'wis-keyaction'

/** Custom DOM events the chrome components listen for. The sidebar now
 *  lives in chrome HTML. */
export const FOCUS_URLBAR_EVENT = 'wis-focus-urlbar'
export const COMMAND_PALETTE_OPEN_EVENT = 'wis-command-palette-open'

const ACCENT_ROTATION: AccentColor[] = [
  'primary',
  'mint',
  'rose',
  'sun',
  'lila',
  'azur',
  'gray',
  'ink',
]

interface KeyActionEvent {
  action: KeyAction
}

export function useKeyActions(): void {
  const { data: state } = useBrowserState()
  const qc = useQueryClient()
  const { requestRename } = usePendingRename()

  const activeWorkspace = state?.workspaces.find((w) => w.id === state.active_workspace_id) ?? null
  const tabs = activeWorkspace ? tabsFor(state!, activeWorkspace) : []
  const activeTab = tabs.find((t) => t.id === activeWorkspace?.active_tab_id) ?? null

  const newWorkspaceInFlight = useRef(false)

  // Track the *previous* active tab so Cmd+0 (Switch to last tab) jumps
  // back to whichever tab was active before the current one, ping-pong
  // style. Updated on every activation that wasn't itself a Cmd+0.
  const previousTabRef = useRef<TabId | null>(null)
  const lastSeenActiveTabRef = useRef<TabId | null>(null)
  useEffect(() => {
    const current = activeTab?.id ?? null
    if (current && current !== lastSeenActiveTabRef.current) {
      previousTabRef.current = lastSeenActiveTabRef.current
      lastSeenActiveTabRef.current = current
    }
  }, [activeTab?.id])

  // Refs hold the latest dispatch context so the Tauri listener
  // always sees current state.
  const stateRef = useRef(state)
  stateRef.current = state
  const activeWorkspaceRef = useRef(activeWorkspace)
  activeWorkspaceRef.current = activeWorkspace
  const tabsRef = useRef(tabs)
  tabsRef.current = tabs
  const activeTabRef = useRef(activeTab)
  activeTabRef.current = activeTab
  const qcRef = useRef(qc)
  qcRef.current = qc
  const requestRenameRef = useRef(requestRename)
  requestRenameRef.current = requestRename

  activeDispatchRef.current = (action) => {
    void handle(action, {
      state: stateRef.current,
      activeWorkspace: activeWorkspaceRef.current,
      tabs: tabsRef.current,
      activeTab: activeTabRef.current,
      previousTabId: previousTabRef.current,
      invalidate: () =>
        qcRef.current.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
      requestRename: requestRenameRef.current,
      newWorkspaceInFlight,
    })
  }

  useEffect(() => {
    ensureKeyActionListener()
  }, [])
}

// ---------------------------------------------------------------------------
// Module-level singleton listener to prevent React 18 StrictMode
// double-mount race.
// ---------------------------------------------------------------------------

type KeyActionGlobals = typeof globalThis & {
  __wisKeyactionDispatch?: (action: KeyAction) => void
  __wisKeyactionListening?: boolean
}
const keyactionGlobals = globalThis as KeyActionGlobals

const activeDispatchRef = {
  set current(fn: ((action: KeyAction) => void) | null) {
    keyactionGlobals.__wisKeyactionDispatch = fn ?? undefined
  },
}

function ensureKeyActionListener(): void {
  if (keyactionGlobals.__wisKeyactionListening) return
  keyactionGlobals.__wisKeyactionListening = true
  void listen<KeyActionEvent>(KEYACTION_EVENT, (e) => {
    keyactionGlobals.__wisKeyactionDispatch?.(e.payload.action)
  })
}

interface DispatchContext {
  state: BrowserState | undefined
  activeWorkspace: Workspace | null
  tabs: Tab[]
  activeTab: Tab | null
  previousTabId: TabId | null
  invalidate: () => void
  requestRename: (id: WorkspaceId) => void
  newWorkspaceInFlight: { current: boolean }
}

async function handle(action: KeyAction, ctx: DispatchContext): Promise<void> {
  switch (action) {
    case 'new_tab': {
      if (!ctx.activeWorkspace) return
      await commands.createTab(ctx.activeWorkspace.id, NEW_TAB_URL)
      ctx.invalidate()
      return
    }
    case 'open_settings': {
      // Switch to an existing settings tab if one is open in the current
      // workspace, else spawn a fresh one.
      if (!ctx.activeWorkspace) return
      const existing = ctx.tabs.find((t) => t.url === SETTINGS_URL)
      if (existing) {
        if (existing.id !== ctx.activeTab?.id) {
          await commands.setActiveTab(existing.id)
          ctx.invalidate()
        }
      } else {
        await commands.createTab(ctx.activeWorkspace.id, SETTINGS_URL)
        ctx.invalidate()
      }
      return
    }
    case 'open_history': {
      // Switch to an existing history tab if one is open, else spawn one.
      if (!ctx.activeWorkspace) return
      const existing = ctx.tabs.find((t) => t.url === HISTORY_URL)
      if (existing) {
        if (existing.id !== ctx.activeTab?.id) {
          await commands.setActiveTab(existing.id)
          ctx.invalidate()
        }
      } else {
        await commands.createTab(ctx.activeWorkspace.id, HISTORY_URL)
        ctx.invalidate()
      }
      return
    }
    case 'clear_browsing_data': {
      window.dispatchEvent(new CustomEvent('wis-open-clear-data'))
      return
    }
    case 'close_tab': {
      if (!ctx.activeTab) return
      closeTabAnimated(ctx.activeTab.id)
      ctx.invalidate()
      return
    }
    case 'new_workspace': {
      if (!ctx.state) return
      if (ctx.newWorkspaceInFlight.current) return
      ctx.newWorkspaceInFlight.current = true
      try {
        const accent = nextAccent(ctx.state.workspaces.map((w) => w.accent))
        const ws = await commands.createWorkspace('New workspace', accent)
        await commands.setActiveWorkspace(ws.id)
        ctx.invalidate()
        ctx.requestRename(ws.id)
      } finally {
        ctx.newWorkspaceInFlight.current = false
      }
      return
    }
    case 'focus_url_bar':
    case 'focus_url_bar_alt': {
      // The URL bar lives in the chrome, so a DOM CustomEvent reaches it.
      await commands.focusWebview('main')
      window.dispatchEvent(new CustomEvent(FOCUS_URLBAR_EVENT, { detail: { select: true } }))
      return
    }
    case 'reload':
      if (ctx.activeTab) await commands.tabReload(ctx.activeTab.id)
      return
    case 'go_back':
    case 'go_back_alt':
      if (ctx.activeTab) await commands.tabGoBack(ctx.activeTab.id)
      return
    case 'go_forward':
    case 'go_forward_alt':
      if (ctx.activeTab) await commands.tabGoForward(ctx.activeTab.id)
      return
    case 'switch_to_tab1':
    case 'switch_to_tab2':
    case 'switch_to_tab3':
    case 'switch_to_tab4':
    case 'switch_to_tab5':
    case 'switch_to_tab6':
    case 'switch_to_tab7':
    case 'switch_to_tab8':
    case 'switch_to_tab9': {
      const index = Number(action.replace('switch_to_tab', '')) - 1
      const tab = ctx.tabs[index]
      if (!tab) return
      await commands.setActiveTab(tab.id)
      ctx.invalidate()
      return
    }
    case 'switch_to_last_tab': {
      const targetId = ctx.previousTabId
      if (!targetId) return
      const target = ctx.tabs.find((t) => t.id === targetId)
      if (!target) return
      await commands.setActiveTab(target.id)
      ctx.invalidate()
      return
    }
    case 'find_in_page':
    case 'find_next':
    case 'find_previous':
      // The find overlay surface listens to `wis-keyaction` directly, and
      // `TabWebViewHost` orchestrates the overlay show/hide. Nothing to
      // do here.
      return
    case 'pop_out_video':
      // `TabWebViewHost` owns the video-popout controller and handles this.
      return
    case 'open_command_palette':
      window.dispatchEvent(new CustomEvent(COMMAND_PALETTE_OPEN_EVENT))
      return
    case 'new_private_window': {
      // TODO: separate OS window
      if (!ctx.state) return
      if (ctx.newWorkspaceInFlight.current) return
      ctx.newWorkspaceInFlight.current = true
      try {
        const accent = nextAccent(ctx.state.workspaces.map((w) => w.accent))
        const ws = await commands.createPrivateWorkspace('Private', accent)
        await commands.createTab(ws.id, NEW_TAB_URL)
        await commands.setActiveWorkspace(ws.id)
        ctx.invalidate()
      } finally {
        ctx.newWorkspaceInFlight.current = false
      }
      return
    }
    case 'new_window':
      // eslint-disable-next-line no-console
      console.warn('Wis: new OS window coming soon')
      return
    case 'split_pane_horizontal':
    case 'split_pane_vertical': {
      if (!ctx.activeWorkspace || !ctx.activeTab) return
      const dir = action === 'split_pane_horizontal' ? 'horizontal' : 'vertical'
      await commands.splitPane(ctx.activeWorkspace.id, dir, false, NEW_TAB_URL)
      ctx.invalidate()
      return
    }
    case 'close_pane': {
      if (!ctx.activeTab) return
      closeTabAnimated(ctx.activeTab.id)
      ctx.invalidate()
      return
    }
    case 'focus_next_pane': {
      const tree = ctx.activeWorkspace?.pane_tree
      if (!tree) return
      const leaves = paneLeaves(tree)
      if (leaves.length < 2) return
      const current = ctx.activeTab?.id ?? null
      const idx = current ? leaves.indexOf(current) : -1
      const next = leaves[(idx + 1) % leaves.length]
      await commands.setActiveTab(next)
      ctx.invalidate()
      return
    }
    case 'focus_pane_left':
    case 'focus_pane_right':
    case 'focus_pane_up':
    case 'focus_pane_down': {
      if (!ctx.activeWorkspace?.pane_tree) return
      const current = ctx.activeTab?.id ?? null
      if (!current) return
      const dir = action.replace('focus_pane_', '') as PaneDirection
      const target = pickPaneInDirection(current, dir)
      if (target && target !== current) {
        await commands.setActiveTab(target)
        ctx.invalidate()
      }
      return
    }
    case 'unsplit_panes': {
      if (!ctx.activeWorkspace) return
      await commands.unsplitPanes(ctx.activeWorkspace.id)
      ctx.invalidate()
      return
    }
    case 'open_downloads': {
      window.dispatchEvent(new CustomEvent(DOWNLOADS_OPEN_EVENT))
      return
    }
    case 'toggle_bookmarks': {
      // Private workspaces have no bookmarks; the glyph is hidden there too.
      if (ctx.activeWorkspace?.ephemeral) return
      window.dispatchEvent(new CustomEvent(BOOKMARKS_OPEN_EVENT))
      return
    }
    case 'bookmark_page': {
      // The URL-bar star owns the bookmark flow (folder memory, auto-pin,
      // picker); this just pushes its button.
      window.dispatchEvent(new CustomEvent(BOOKMARK_PAGE_EVENT))
      return
    }
    case 'pin_tab': {
      if (!ctx.activeTab || !ctx.activeWorkspace) return
      const pinned = ctx.activeWorkspace.pinned_tab_ids.includes(ctx.activeTab.id)
      await commands.pinTab(ctx.activeTab.id, !pinned)
      ctx.invalidate()
      return
    }
    case 'duplicate_tab': {
      if (!ctx.activeTab || !ctx.activeWorkspace) return
      await commands.createTab(ctx.activeWorkspace.id, ctx.activeTab.url)
      ctx.invalidate()
      return
    }
    case 'copy_url': {
      if (!ctx.activeTab) return
      await commands.copyText(ctx.activeTab.url)
      return
    }
    case 'element_picker': {
      if (ctx.activeTab) await commands.startElementPicker(ctx.activeTab.id)
      return
    }
    case 'toggle_sidebar': {
      const settings = ctx.state?.settings
      if (!settings || settings.layout === 'top_tabs') return
      await commands.updateSettings({ sidebar_collapsible: !settings.sidebar_collapsible })
      ctx.invalidate()
      return
    }
    case 'restore_closed_tab': {
      await commands.restoreClosedTab()
      ctx.invalidate()
      return
    }
    case 'next_workspace':
    case 'previous_workspace': {
      const st = ctx.state
      if (!st) return
      const list = st.workspaces
      const idx = list.findIndex((w) => w.id === st.active_workspace_id)
      if (idx < 0) return
      const target = action === 'next_workspace' ? idx + 1 : idx - 1
      if (target < 0 || target >= list.length) return // clamp: no-op at the ends
      await commands.setActiveWorkspace(list[target].id)
      ctx.invalidate()
      return
    }
    case 'next_tab':
    case 'previous_tab': {
      const list = ctx.tabs
      const len = list.length
      if (len === 0) return
      const idx = ctx.activeTab ? list.findIndex((t) => t.id === ctx.activeTab!.id) : -1
      const target = action === 'next_tab' ? (idx + 1) % len : (idx - 1 + len) % len
      const tab = list[target]
      if (!tab) return
      await commands.setActiveTab(tab.id)
      ctx.invalidate()
      return
    }
    case 'hard_reload':
      if (ctx.activeTab) await commands.tabHardReload(ctx.activeTab.id)
      return
    case 'zoom_in':
    case 'zoom_out':
    case 'zoom_reset': {
      if (!ctx.activeTab) return
      const kind = action === 'zoom_in' ? 'in' : action === 'zoom_out' ? 'out' : 'reset'
      await commands.tabZoom(ctx.activeTab.id, kind)
      return
    }
  }
}

type PaneDirection = 'left' | 'right' | 'up' | 'down'

/**
 * Pick the nearest pane in `dir` from the focused pane, using the rendered
 * `[data-pane-leaf]` rects. Favours panes directly in the direction (small
 * perpendicular offset). Returns null if none lie that way.
 */
function pickPaneInDirection(currentId: TabId, dir: PaneDirection): TabId | null {
  const rects = Array.from(
    document.querySelectorAll<HTMLElement>('[data-pane-leaf]'),
  )
    .map((el) => ({ id: el.getAttribute('data-pane-leaf') as TabId | null, rect: el.getBoundingClientRect() }))
    .filter((r): r is { id: TabId; rect: DOMRect } => r.id !== null)

  const cur = rects.find((r) => r.id === currentId)
  if (!cur) return null
  const fx = cur.rect.left + cur.rect.width / 2
  const fy = cur.rect.top + cur.rect.height / 2

  let best: TabId | null = null
  let bestScore = Infinity
  for (const r of rects) {
    if (r.id === currentId) continue
    const dx = r.rect.left + r.rect.width / 2 - fx
    const dy = r.rect.top + r.rect.height / 2 - fy
    let inDir = false
    let primary = 0
    let secondary = 0
    if (dir === 'left') {
      inDir = dx < -1
      primary = -dx
      secondary = Math.abs(dy)
    } else if (dir === 'right') {
      inDir = dx > 1
      primary = dx
      secondary = Math.abs(dy)
    } else if (dir === 'up') {
      inDir = dy < -1
      primary = -dy
      secondary = Math.abs(dx)
    } else {
      inDir = dy > 1
      primary = dy
      secondary = Math.abs(dx)
    }
    if (!inDir) continue
    const score = primary + secondary * 2
    if (score < bestScore) {
      bestScore = score
      best = r.id
    }
  }
  return best
}

/** Pane leaves left-to-right / top-to-bottom (mirrors `PaneNode::leaves`). */
function paneLeaves(node: PaneNode): TabId[] {
  if (node.kind === 'leaf') return [node.tab]
  return [...paneLeaves(node.first), ...paneLeaves(node.second)]
}

function tabsFor(state: BrowserState, workspace: Workspace): Tab[] {
  return workspace.tab_order
    .map((id) => state.tabs.find((t) => t.id === id))
    .filter((t): t is Tab => t !== undefined)
}

function nextAccent(used: AccentColor[]): AccentColor {
  for (const accent of ACCENT_ROTATION) {
    if (!used.includes(accent)) return accent
  }
  return ACCENT_ROTATION[used.length % ACCENT_ROTATION.length]
}
