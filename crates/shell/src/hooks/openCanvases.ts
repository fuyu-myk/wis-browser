import { commands } from '@/bindings/commands'
import type { Tab, TabId, WorkspaceId } from '@/bindings/types'
import { guardFeatureUnavailable } from '@/lib/platformSupport'
import { CANVAS_URL, isCanvasUrl } from '@/lib/url'

/** Switch to the workspace's canvas tab, or open one. Resolves once done. */
export async function openCanvases(
  workspaceId: WorkspaceId,
  tabs: readonly Tab[],
  activeTabId: TabId | null,
): Promise<void> {
  if (guardFeatureUnavailable('canvases')) return
  const existing = tabs.find((t) => t.workspace_id === workspaceId && isCanvasUrl(t.url))
  if (existing === undefined) await commands.createTab(workspaceId, CANVAS_URL)
  else if (existing.id !== activeTabId) await commands.setActiveTab(existing.id)
}
