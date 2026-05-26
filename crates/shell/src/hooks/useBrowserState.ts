import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { BrowserState, UserSettings, UserSettingsPatch } from '@/bindings/types'

export const BROWSER_STATE_KEY = ['browser-state'] as const

export function useBrowserState() {
  return useQuery<BrowserState>({
    queryKey: BROWSER_STATE_KEY,
    queryFn: () => commands.getBrowserState(),
    staleTime: Infinity,
    retry: 0,
  })
}

/**
 * Convenience: pull `UserSettings` off the browser state. Returns
 * `undefined` while the initial query is still loading.
 */
export function useSettings(): UserSettings | undefined {
  const { data } = useBrowserState()
  return data?.settings
}

export function useUpdateSettings() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (patch: UserSettingsPatch) => commands.updateSettings(patch),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY })
    },
  })
}
