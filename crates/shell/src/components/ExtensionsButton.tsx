import { useQuery } from '@tanstack/react-query'
import { useRef } from 'react'

import { commands } from '@/bindings/commands'
import type { WorkspaceId } from '@/bindings/types'
import { PuzzleIcon } from '@/components/settingsIcons'
import { useBrowserState } from '@/hooks/useBrowserState'
import { EXT_BUTTON_ATTR, useExtensionsMenu } from '@/hooks/useExtensionsMenu'

export function ExtensionsButton({ workspaceId }: { workspaceId: WorkspaceId | null }) {
  const { data: state } = useBrowserState()
  const { data: supported } = useQuery({
    queryKey: ['extensions-supported'],
    queryFn: () => commands.extensionsSupported(),
    staleTime: Infinity,
  })
  const buttonRef = useRef<HTMLButtonElement>(null)
  const menu = useExtensionsMenu(workspaceId)

  if (!supported || !state?.settings.extensions.length) return null
  return (
    <button
      ref={buttonRef}
      type="button"
      className="kb-urlbar-key"
      title="Extensions"
      aria-label="Extensions"
      {...{ [EXT_BUTTON_ATTR]: '' }}
      onMouseDown={(e) => {
        e.preventDefault()
        e.stopPropagation()
      }}
      onClick={() => buttonRef.current && menu.toggle(buttonRef.current)}
    >
      <PuzzleIcon />
    </button>
  )
}
