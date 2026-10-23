import type { PrivacyMode } from '@/bindings/types'
import { useBrowserStateSelector } from '@/hooks/useBrowserState'

/**
 * The active workspace's privacy mode, or `null` while state is loading.
 *
 * Selector-based: always-mounted chrome that reads this (e.g. the window
 * brand) re-renders only when the privacy mode actually changes, not on every
 * tab navigation or loading flip.
 */
export function useActivePrivacyMode(): PrivacyMode | null {
  return (
    useBrowserStateSelector((s) => {
      const ws = s.workspaces.find((w) => w.id === s.active_workspace_id)
      return ws?.privacy_mode ?? null
    }) ?? null
  )
}
