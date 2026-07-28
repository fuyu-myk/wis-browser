/**
 * Resolve address-bar suggestion URLs to already-open tabs and switch to
 * them. Shared by every surface that mounts the suggestions dropdown (the
 * top URL bar and the launchpad omnisearch).
 */

import { useCallback } from 'react'
import { useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { TabId } from '@/bindings/types'
import { BROWSER_STATE_KEY, useBrowserState } from '@/hooks/useBrowserState'

/** Canonical form for matching a suggestion URL against an open tab's URL. */
function canonUrl(url: string): string {
  return url
    .replace(/^https?:\/\//, '')
    .replace(/^www\./, '')
    .replace(/\/$/, '')
    .toLowerCase()
}

export interface OpenTabActions {
  /** Resolve a suggestion URL to an already-open tab id, or null. */
  findOpenTab: (url: string) => TabId | null
  /** Switch focus to an already-open tab, hopping workspaces if needed. */
  switchToTab: (id: TabId) => void
}

export function useOpenTabActions(): OpenTabActions {
  const { data: state } = useBrowserState()
  const qc = useQueryClient()

  const findOpenTab = useCallback(
    (url: string): TabId | null => {
      const target = canonUrl(url)
      return state?.tabs.find((t) => canonUrl(t.url) === target)?.id ?? null
    },
    [state?.tabs],
  )

  const switchToTab = useCallback(
    (id: TabId) => {
      const tab = state?.tabs.find((t) => t.id === id)
      if (!tab) return
      const run = async () => {
        if (tab.workspace_id !== state?.active_workspace_id) {
          await commands.setActiveWorkspace(tab.workspace_id).catch(() => undefined)
        }
        await commands.setActiveTab(id).catch(() => undefined)
        void qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY })
      }
      void run()
    },
    [state?.tabs, state?.active_workspace_id, qc],
  )

  return { findOpenTab, switchToTab }
}
