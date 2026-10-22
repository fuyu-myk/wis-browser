import { useQuery } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'

export const VAULT_STATUS_KEY = ['vault-status'] as const

export function useVaultStatus() {
  return useQuery({
    queryKey: VAULT_STATUS_KEY,
    queryFn: () => commands.vaultStatus(),
    staleTime: 0,
    refetchOnMount: 'always',
  })
}
