import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { Tab, TabFolderId, WorkspaceId } from '@/bindings/types'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'

/**
 * Keeps the "new tab target" folder in step with the focused tab: clicking a
 * tab inside a folder points the target at that folder, clicking a loose or
 * pinned tab clears it. Shared by the sidebar and top-tabs strips so both
 * behave identically.
 */
export function useFolderTarget(
  workspaceId: WorkspaceId,
  selectedFolderId: TabFolderId | null,
) {
  const qc = useQueryClient()
  const setTarget = useMutation({
    mutationFn: (target: TabFolderId | null) =>
      commands.selectTabFolder(workspaceId, target),
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })

  // Point the target at the tab's folder (null for a loose/pinned tab); a no-op
  // when it already matches.
  const syncToTab = (tab: Tab) => {
    const target = tab.folder_id ?? null
    if (target !== selectedFolderId) setTarget.mutate(target)
  }

  return { setTarget: setTarget.mutate, syncToTab }
}
