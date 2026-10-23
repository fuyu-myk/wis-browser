import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { TabFolderId, WorkspaceId } from '@/bindings/types'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'

/**
 * Sets the "new tab target" folder from the chrome (folder header click, bare
 * strip click). Focusing a tab moves the target in core, so the strips only
 * report explicit choices here.
 */
export function useFolderTarget(workspaceId: WorkspaceId) {
  const qc = useQueryClient()
  const setTarget = useMutation({
    mutationFn: (target: TabFolderId | null) =>
      commands.selectTabFolder(workspaceId, target),
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })

  return { setTarget: setTarget.mutate }
}
