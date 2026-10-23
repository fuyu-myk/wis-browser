import { useQuery } from '@tanstack/react-query'
import { useRef } from 'react'

import { commands } from '@/bindings/commands'
import type { ExtensionAction, TabId, WorkspaceId } from '@/bindings/types'
import { PuzzleIcon } from '@/components/settingsIcons'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useExtensionActions } from '@/features/extensions/hooks/useExtensionActions'
import { EXT_BUTTON_ATTR, useExtensionsMenu } from '@/features/extensions/hooks/useExtensionsMenu'
import { isStoreItemPage } from '@/features/extensions/lib/extensionsMenu'

interface ExtensionsButtonProps {
  workspaceId: WorkspaceId | null
  activeTabId: TabId | null
}

const keepFocus = (e: React.MouseEvent) => {
  e.preventDefault()
  e.stopPropagation()
}

function PinnedAction({ action }: { action: ExtensionAction }) {
  const buttonRef = useRef<HTMLButtonElement>(null)
  const label = action.label || action.name

  const run = async () => {
    const button = buttonRef.current
    if (!button || (await commands.extensionDismissPopup()) === action.id) return
    const rect = button.getBoundingClientRect()
    await commands.extensionRun(action.id, rect.x, rect.y, rect.width, rect.height)
  }

  return (
    <button
      ref={buttonRef}
      type="button"
      className="kb-urlbar-key kb-ext-pinned"
      title={label}
      aria-label={label}
      disabled={!action.enabled}
      onMouseDown={keepFocus}
      onClick={() => void run().catch(() => undefined)}
    >
      {action.icon ? <img className="kb-ext-pinned-icon" src={action.icon} alt="" /> : <PuzzleIcon />}
      {action.badge && <span className="kb-ext-pinned-badge">{action.badge}</span>}
    </button>
  )
}

export function ExtensionsButton({ workspaceId, activeTabId }: ExtensionsButtonProps) {
  const { data: state } = useBrowserState()
  const { data: supported } = useQuery({
    queryKey: ['extensions-supported'],
    queryFn: () => commands.extensionsSupported(),
    staleTime: Infinity,
  })
  const buttonRef = useRef<HTMLButtonElement>(null)
  const menu = useExtensionsMenu(workspaceId, buttonRef)
  const installed = state?.settings.extensions ?? []
  const pinned = new Set(installed.filter((e) => e.pinned).map((e) => e.id))
  const actions = useExtensionActions(workspaceId, activeTabId, !!supported && pinned.size > 0)

  const activeUrl = state?.tabs.find((t) => t.id === activeTabId)?.url ?? ''
  if (!supported || (installed.length === 0 && !isStoreItemPage(activeUrl))) return null
  return (
    <>
      {actions
        .filter((action) => pinned.has(action.id))
        .map((action) => (
          <PinnedAction key={action.id} action={action} />
        ))}
      <button
        ref={buttonRef}
        type="button"
        className="kb-urlbar-key"
        title="Extensions"
        aria-label="Extensions"
        {...{ [EXT_BUTTON_ATTR]: '' }}
        onMouseDown={keepFocus}
        onClick={() => buttonRef.current && menu.toggle(buttonRef.current)}
      >
        <PuzzleIcon />
      </button>
    </>
  )
}
