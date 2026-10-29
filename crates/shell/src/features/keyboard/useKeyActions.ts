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
import { isPeekOpen } from '@/features/page-tools/peekState'
import { isWindowFocused } from '@/lib/windowFocus'
import { nextAccent } from '@/lib/accents'
import { closeTabAnimated } from '@/features/tabs/lib/tabClose'
import { noteActiveTab, previousTab, pruneTabMru } from '@/features/tabs/lib/tabMru'
import { DOWNLOADS_OPEN_EVENT } from '@/features/downloads/downloads'
import { BOOKMARK_PAGE_EVENT, BOOKMARKS_OPEN_EVENT } from '@/features/bookmarks/lib/bookmarkMenu'
import type { BrowserState, KeyAction, PaneNode, Tab, TabId, Workspace, WorkspaceId } from '@/bindings/types'
import { HISTORY_URL, NEW_TAB_URL, SETTINGS_URL, isChromeUrl, resolveNewTabUrl } from '@/lib/url'
import { SCREENSHOT_CAPTURED_EVENT, SCREENSHOT_REGION_OPEN_EVENT } from '@/features/screenshot/screenshot'
import { BROWSER_STATE_KEY, useBrowserState } from '@/hooks/useBrowserState'
import { usePendingRename } from '@/features/workspaces/hooks/usePendingRename'
import { currentPlatform } from '@/lib/platform'
import { guardFeatureUnavailable } from '@/lib/platformSupport'
import { errorMessage } from '@/lib/errors'
import { showToast } from '@/lib/toast'
import { openCanvases } from '@/features/canvas/hooks/openCanvases'
import { useQueryClient } from '@tanstack/react-query'
import { WINDOW_PICKER_OPEN_EVENT } from '@/features/workspaces/hooks/useWindowPickerOverlay'
import { currentArea, focusArea, nextArea } from '@/features/keyboard/lib/focusAreas'

const KEYACTION_EVENT = 'wis-keyaction'

/** Custom DOM events the chrome components listen for. The sidebar now
 *  lives in chrome HTML. */
export const FOCUS_URLBAR_EVENT = 'wis-focus-urlbar'
export const COMMAND_PALETTE_OPEN_EVENT = 'wis-command-palette-open'
/** DOM event that opens the New Window free-workspace picker overlay. */

interface KeyActionEvent {
  action: KeyAction
  /** Source tab when dispatched by the Windows page forwarder (`chrome/keybind.rs`);
   *  absent for native menu / chrome-emitted actions. */
  tab?: string
}

export function useKeyActions(): void {
  const { data: state } = useBrowserState()
  const qc = useQueryClient()
  const { requestRename } = usePendingRename()

  const activeWorkspace = state?.workspaces.find((w) => w.id === state.active_workspace_id) ?? null
  const tabs = activeWorkspace ? tabsFor(state!, activeWorkspace) : []
  const numberedTabs = activeWorkspace ? numberedTabsFor(state!, activeWorkspace) : []
  const activeTab = tabs.find((t) => t.id === activeWorkspace?.active_tab_id) ?? null

  const newWorkspaceInFlight = useRef(false)
  // Where toggle_private_workspace returns to from a private workspace.
  const lastRegularWorkspace = useRef<WorkspaceId | null>(null)
  if (activeWorkspace && !activeWorkspace.ephemeral) lastRegularWorkspace.current = activeWorkspace.id

  // Track the *previous* active tab (lib/tabMru) so Switch-to-last-tab jumps
  // ping-pong style and closing the active tab returns to it (tabClose.ts).
  useEffect(() => {
    noteActiveTab(activeTab?.id ?? null, activeWorkspace?.id ?? null)
  }, [activeTab?.id, activeWorkspace?.id])
  useEffect(() => {
    if (!state) return
    const live = new Set(state.tabs.map((t) => t.id))
    pruneTabMru((id) => live.has(id))
  }, [state])

  // Refs hold the latest dispatch context so the Tauri listener
  // always sees current state.
  const stateRef = useRef(state)
  stateRef.current = state
  const activeWorkspaceRef = useRef(activeWorkspace)
  activeWorkspaceRef.current = activeWorkspace
  const tabsRef = useRef(tabs)
  tabsRef.current = tabs
  const numberedTabsRef = useRef(numberedTabs)
  numberedTabsRef.current = numberedTabs
  const activeTabRef = useRef(activeTab)
  activeTabRef.current = activeTab
  const qcRef = useRef(qc)
  qcRef.current = qc
  const requestRenameRef = useRef(requestRename)
  requestRenameRef.current = requestRename

  activeDispatchRef.current = (action, sourceTab) => {
    if (sourceTab && !tabsRef.current.some((t) => t.id === sourceTab)) return
    void handle(action, {
      state: stateRef.current,
      activeWorkspace: activeWorkspaceRef.current,
      tabs: tabsRef.current,
      numberedTabs: numberedTabsRef.current,
      activeTab: activeTabRef.current,
      previousTabId: previousTab(),
      invalidate: () =>
        qcRef.current.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
      requestRename: requestRenameRef.current,
      newWorkspaceInFlight,
      lastRegularWorkspace: lastRegularWorkspace.current,
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
  __wisKeyactionDispatch?: (action: KeyAction, sourceTab?: string) => void
  __wisKeyactionListening?: boolean
}
const keyactionGlobals = globalThis as KeyActionGlobals

const activeDispatchRef = {
  set current(fn: ((action: KeyAction, sourceTab?: string) => void) | null) {
    keyactionGlobals.__wisKeyactionDispatch = fn ?? undefined
  },
}

function ensureKeyActionListener(): void {
  if (keyactionGlobals.__wisKeyactionListening) return
  keyactionGlobals.__wisKeyactionListening = true
  void listen<KeyActionEvent>(KEYACTION_EVENT, (e) => {
    if (!isWindowFocused()) return
    keyactionGlobals.__wisKeyactionDispatch?.(e.payload.action, e.payload.tab)
  })
}

interface DispatchContext {
  state: BrowserState | undefined
  activeWorkspace: Workspace | null
  tabs: Tab[]
  /** Visible-order, pinned-excluded tab list for Cmd+1..9 numbering. */
  numberedTabs: Tab[]
  activeTab: Tab | null
  previousTabId: TabId | null
  invalidate: () => void
  requestRename: (id: WorkspaceId) => void
  newWorkspaceInFlight: { current: boolean }
  lastRegularWorkspace: WorkspaceId | null
}

async function handle(action: KeyAction, ctx: DispatchContext): Promise<void> {
  // A feature with no working backend on this platform raises a
  // "not available on {target}" toast.
  if (action !== 'pop_out_video' && guardFeatureUnavailable(action)) return
  switch (action) {
    case 'new_tab': {
      if (!ctx.activeWorkspace) return
      await commands.createTab(ctx.activeWorkspace.id, resolveNewTabUrl(ctx.state?.settings))
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
    case 'open_canvases': {
      if (!ctx.activeWorkspace) return
      await openCanvases(ctx.activeWorkspace.id, ctx.tabs, ctx.activeTab?.id ?? null)
      ctx.invalidate()
      return
    }
    case 'clear_browsing_data': {
      window.dispatchEvent(new CustomEvent('wis-open-clear-data'))
      return
    }
    case 'close_tab': {
      if (isPeekOpen()) return
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
      if (isPeekOpen()) return
      // The URL bar lives in the chrome, so a DOM CustomEvent reaches it.
      await commands.focusSelf()
      window.dispatchEvent(new CustomEvent(FOCUS_URLBAR_EVENT, { detail: { select: true } }))
      return
    }
    case 'focus_next_area':
    case 'focus_previous_area': {
      if (isPeekOpen()) return
      const area = nextArea(currentArea(), action === 'focus_previous_area')
      await focusArea(area, ctx.activeTab ?? null, () =>
        window.dispatchEvent(new CustomEvent(FOCUS_URLBAR_EVENT, { detail: { select: true } })),
      )
      return
    }
    case 'reload':
      if (isPeekOpen()) return
      if (ctx.activeTab) await commands.tabReload(ctx.activeTab.id)
      return
    case 'go_back':
    case 'go_back_alt':
      if (isPeekOpen()) return
      if (ctx.activeTab) await commands.tabGoBack(ctx.activeTab.id)
      return
    case 'go_forward':
    case 'go_forward_alt':
      if (isPeekOpen()) return
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
      const tab = ctx.numberedTabs[index]
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
    case 'inspect': {
      const id = ctx.state?.workspaces.find(
        (w) => w.id === ctx.state?.active_workspace_id,
      )?.active_tab_id
      if (id) void commands.inspectTab(id).catch(() => undefined)
      return
    }
    case 'open_command_palette':
      if (isPeekOpen()) return
      window.dispatchEvent(new CustomEvent(COMMAND_PALETTE_OPEN_EVENT))
      return
    case 'new_private_window': {
      // A new private window is always a fresh ephemeral workspace in its own
      // OS window.
      if (!ctx.state) return
      if (ctx.newWorkspaceInFlight.current) return
      ctx.newWorkspaceInFlight.current = true
      try {
        const accent = nextAccent(ctx.state.workspaces.map((w) => w.accent))
        const ws = await commands.createPrivateWorkspace('Private', accent)
        await commands.createTab(ws.id, NEW_TAB_URL)
        await commands.newWindow(ws.id)
      } finally {
        ctx.newWorkspaceInFlight.current = false
      }
      return
    }
    case 'new_window':
      // Open the free-workspace picker; selecting one (or "new empty
      // workspace") spawns a new OS window bound to it.
      window.dispatchEvent(new CustomEvent(WINDOW_PICKER_OPEN_EVENT))
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
    case 'print': {
      if (!ctx.activeTab || isChromeUrl(ctx.activeTab.url)) return
      await commands.printActiveTab(ctx.activeTab.id)
      return
    }
    case 'capture_region': {
      // Window-region: open the full-window selection overlay (inclusive of
      // the chrome). The overlay hook performs the crop capture on a result.
      if (!ctx.activeTab) return
      // Windows and Linux crop the page viewport, which chrome pages lack;
      // skip like capture_visible.
      if (currentPlatform() !== 'macos' && isChromeUrl(ctx.activeTab.url)) return
      window.dispatchEvent(
        new CustomEvent(SCREENSHOT_REGION_OPEN_EVENT, { detail: { tabId: ctx.activeTab.id } }),
      )
      return
    }
    case 'capture_visible':
    case 'capture_window': {
      if (!ctx.activeTab) return
      // Visible snapshots a `tab-*` webview, which chrome pages don't have.
      // Window capture grabs the whole frame only on macOS; Windows and Linux
      // fall back to the same tab snapshot, so they skip chrome pages there too.
      if (action === 'capture_visible' && isChromeUrl(ctx.activeTab.url)) return
      if (
        action === 'capture_window' &&
        currentPlatform() !== 'macos' &&
        isChromeUrl(ctx.activeTab.url)
      )
        return
      const mode = action === 'capture_window' ? 'window' : 'visible'
      try {
        const captured = await commands.captureToClipboard(ctx.activeTab.id, mode)
        // A capture copies to the clipboard; the success toast offers Save.
        if (captured) window.dispatchEvent(new CustomEvent(SCREENSHOT_CAPTURED_EVENT))
      } catch (err) {
        // eslint-disable-next-line no-console
        console.error('captureToClipboard failed', err)
      }
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
    case 'toggle_private_workspace': {
      const st = ctx.state
      if (!st || !ctx.activeWorkspace) return
      if (ctx.activeWorkspace.ephemeral) {
        const back =
          st.workspaces.find((w) => w.id === ctx.lastRegularWorkspace && !w.ephemeral) ??
          st.workspaces.find((w) => !w.ephemeral)
        if (back) await commands.setActiveWorkspace(back.id)
        ctx.invalidate()
        return
      }
      const existing = [...st.workspaces].reverse().find((w) => w.ephemeral)
      if (existing) {
        await commands.setActiveWorkspace(existing.id)
        ctx.invalidate()
        return
      }
      if (ctx.newWorkspaceInFlight.current) return
      ctx.newWorkspaceInFlight.current = true
      try {
        const accent = nextAccent(st.workspaces.map((w) => w.accent))
        const ws = await commands.createPrivateWorkspace('Private', accent)
        await commands.createTab(ws.id, NEW_TAB_URL)
        await commands.setActiveWorkspace(ws.id)
        ctx.invalidate()
      } finally {
        ctx.newWorkspaceInFlight.current = false
      }
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
      const len = list.length
      if (len === 0) return
      const idx = list.findIndex((w) => w.id === st.active_workspace_id)
      if (idx < 0) return
      const target = action === 'next_workspace' ? (idx + 1) % len : (idx - 1 + len) % len
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
      if (isPeekOpen()) return
      if (ctx.activeTab) await commands.tabHardReload(ctx.activeTab.id)
      return
    case 'toggle_reader': {
      if (!ctx.activeTab || isChromeUrl(ctx.activeTab.url)) return
      await commands.tabSetReader(ctx.activeTab.id, !ctx.activeTab.reader)
      ctx.invalidate()
      return
    }
    case 'toggle_translation': {
      if (!ctx.activeTab || isChromeUrl(ctx.activeTab.url)) return
      try {
        await commands.tabSetTranslated(ctx.activeTab.id, !ctx.activeTab.translated)
      } catch (err) {
        showToast(errorMessage(err))
      }
      ctx.invalidate()
      return
    }
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

/**
 * Tabs in the order Cmd+1..9 number them: the on-screen reading order, so
 * reordering and filing into folders both renumber. Members of expanded
 * folders (folders by position) come first, then loose tabs; pinned tabs and
 * collapsed-folder members are excluded since neither is a numbered visible
 * slot. Plain tab cycling stays on the full `tabsFor` set instead.
 */
function numberedTabsFor(state: BrowserState, workspace: Workspace): Tab[] {
  const ordered = tabsFor(state, workspace)
  const pinned = new Set(workspace.pinned_tab_ids)
  const candidates = ordered.filter((t) => !pinned.has(t.id))
  const folders = [...workspace.tab_folders]
    .filter((f) => !f.collapsed)
    .sort((a, b) => a.position - b.position)
  const foldered = folders.flatMap((f) => candidates.filter((t) => t.folder_id === f.id))
  const loose = candidates.filter((t) => t.folder_id == null)
  return [...foldered, ...loose]
}
