import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from 'react'

import type { WorkspaceId } from '@/bindings/types'

interface PendingRenameContextValue {
  /** Workspace id whose pill should swap to an inline rename input, or `null`. */
  pendingRenameWorkspaceId: WorkspaceId | null
  /** Enter rename mode for the given workspace. */
  requestRename: (id: WorkspaceId) => void
  /** Leave rename mode without committing. The input keeps the placeholder name. */
  clearRename: () => void
}

const PendingRenameContext = createContext<PendingRenameContextValue | null>(null)

interface PendingRenameProviderProps {
  children: ReactNode
}

export function PendingRenameProvider({ children }: PendingRenameProviderProps) {
  const [pendingRenameWorkspaceId, setPending] = useState<WorkspaceId | null>(null)
  const requestRename = useCallback((id: WorkspaceId) => setPending(id), [])
  const clearRename = useCallback(() => setPending(null), [])

  const value = useMemo<PendingRenameContextValue>(
    () => ({ pendingRenameWorkspaceId, requestRename, clearRename }),
    [pendingRenameWorkspaceId, requestRename, clearRename],
  )

  return <PendingRenameContext.Provider value={value}>{children}</PendingRenameContext.Provider>
}

/**
 * Consumer hook. Returns a default no-op value when no provider is mounted
 * so unit-testing individual components doesn't require the provider.
 */
export function usePendingRename(): PendingRenameContextValue {
  const ctx = useContext(PendingRenameContext)
  if (ctx) return ctx
  return {
    pendingRenameWorkspaceId: null,
    requestRename: () => undefined,
    clearRename: () => undefined,
  }
}
