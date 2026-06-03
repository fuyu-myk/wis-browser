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
import type { AccentColor, BrowserState, KeyAction, Tab, TabId, Workspace, WorkspaceId } from '@/bindings/types'
import { NEW_TAB_URL, SETTINGS_URL } from '@/lib/url'
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

const activeDispatchRef: { current: ((action: KeyAction) => void) | null } = {
  current: null,
}

let keyActionListenerEnsured = false

function ensureKeyActionListener(): void {
  if (keyActionListenerEnsured) return
  keyActionListenerEnsured = true
  void listen<KeyActionEvent>(KEYACTION_EVENT, (e) => {
    activeDispatchRef.current?.(e.payload.action)
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
    case 'close_tab': {
      if (!ctx.activeTab) return
      await commands.closeTab(ctx.activeTab.id)
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
  }
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
