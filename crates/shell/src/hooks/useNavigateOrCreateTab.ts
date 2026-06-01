import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { Tab, WorkspaceId } from '@/bindings/types'
import { isChromeUrl } from '@/lib/url'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'

/**
 * Navigate the placeholder chrome tab when one is active, otherwise spawn a
 * fresh tab in the workspace. Shared between LaunchPad quicklinks, the
 * settings cog, and keyboard shortcuts so the URL bar always reflects the
 * latest navigation regardless of entry point.
 */
export function useNavigateOrCreateTab(
  workspaceId: WorkspaceId | null,
  placeholderTab: Tab | null,
): (url: string) => void {
  const qc = useQueryClient()
  const navigateMutation = useMutation({
    mutationFn: ({ id, url }: { id: Tab['id']; url: string }) =>
      commands.navigateTab(id, url),
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })
  const createMutation = useMutation({
    mutationFn: ({ workspaceId, url }: { workspaceId: WorkspaceId; url: string }) =>
      commands.createTab(workspaceId, url),
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })

  return (url) => {
    if (placeholderTab && isChromeUrl(placeholderTab.url)) {
      navigateMutation.mutate({ id: placeholderTab.id, url })
    } else if (workspaceId) {
      createMutation.mutate({ workspaceId, url })
    }
  }
}
