import { useEffect } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { ExtensionAction, TabId, WorkspaceId } from '@/bindings/types'

const ACTIONS_KEY = 'extension-actions'
const ACTIONS_CHANGED_EVENT = 'wis-extension-actions-changed'
const EXTENSIONS_CHANGED_EVENT = 'wis-extensions-changed'

/** The actions running in this window's workspace for its active tab, kept
 *  current as badges change and extensions come and go. */
export function useExtensionActions(
  workspaceId: WorkspaceId | null,
  activeTabId: TabId | null,
  enabled: boolean,
): ExtensionAction[] {
  const qc = useQueryClient()
  const { data = [] } = useQuery({
    queryKey: [ACTIONS_KEY, workspaceId, activeTabId],
    queryFn: () => commands.extensionActions(),
    enabled,
  })

  useEffect(() => {
    if (!enabled) return
    const refresh = () => void qc.invalidateQueries({ queryKey: [ACTIONS_KEY] })
    const unlisteners: UnlistenFn[] = []
    let cancelled = false
    const keep = (fn: UnlistenFn) => {
      if (cancelled) fn()
      else unlisteners.push(fn)
    }
    void listen<WorkspaceId>(ACTIONS_CHANGED_EVENT, (e) => {
      if (e.payload === workspaceId) refresh()
    }).then(keep)
    void listen(EXTENSIONS_CHANGED_EVENT, refresh).then(keep)
    return () => {
      cancelled = true
      for (const fn of unlisteners) fn()
    }
  }, [enabled, workspaceId, qc])

  return data
}
