import type { PrivacyMode } from '@/bindings/types'
import { useBrowserState } from '@/hooks/useBrowserState'

/**
 * The active workspace's privacy mode, or `null` while state is loading.
 */
export function useActivePrivacyMode(): PrivacyMode | null {
  const { data: state } = useBrowserState()
  if (!state) return null
  const ws = state.workspaces.find((w) => w.id === state.active_workspace_id)
  return ws?.privacy_mode ?? null
}
