// Typed wrappers around `@tauri-apps/api/core` invoke() for every command
// exposed by `crates/shell/src-tauri/src/commands.rs`.
//
// The wrappers accept camelCase arguments and translate at the boundary to
// the snake_case keys serde expects.

import { invoke } from '@tauri-apps/api/core'
import type {
  AccentColor,
  BrowserState,
  Tab,
  TabId,
  UserSettings,
  UserSettingsPatch,
  Workspace,
  WorkspaceId,
} from './types'

export const commands = {
  // -- queries --------------------------------------------------------------

  async getBrowserState(): Promise<BrowserState> {
    return invoke('get_browser_state')
  },

  async getWorkspace(id: WorkspaceId): Promise<Workspace> {
    return invoke('get_workspace', { id })
  },

  async getTab(id: TabId): Promise<Tab> {
    return invoke('get_tab', { id })
  },

  async getSettings(): Promise<UserSettings> {
    return invoke('get_settings')
  },

  // -- workspace mutations --------------------------------------------------

  async createWorkspace(name: string, accent: AccentColor): Promise<Workspace> {
    return invoke('create_workspace', { name, accent })
  },

  async renameWorkspace(id: WorkspaceId, name: string): Promise<Workspace> {
    return invoke('rename_workspace', { id, name })
  },

  async deleteWorkspace(id: WorkspaceId): Promise<void> {
    return invoke('delete_workspace', { id })
  },

  async setActiveWorkspace(id: WorkspaceId): Promise<void> {
    return invoke('set_active_workspace', { id })
  },

  // -- tab mutations --------------------------------------------------------

  async createTab(workspaceId: WorkspaceId, url: string): Promise<Tab> {
    return invoke('create_tab', { workspace_id: workspaceId, url })
  },

  async closeTab(id: TabId): Promise<void> {
    return invoke('close_tab', { id })
  },

  async reorderTabs(workspaceId: WorkspaceId, order: TabId[]): Promise<void> {
    return invoke('reorder_tabs', { workspace_id: workspaceId, order })
  },

  async pinTab(id: TabId, pinned: boolean): Promise<Tab> {
    return invoke('pin_tab', { id, pinned })
  },

  async setActiveTab(id: TabId): Promise<void> {
    return invoke('set_active_tab', { id })
  },

  // -- settings -------------------------------------------------------------

  async updateSettings(patch: UserSettingsPatch): Promise<UserSettings> {
    return invoke('update_settings', { patch })
  },
} as const

export type Commands = typeof commands
