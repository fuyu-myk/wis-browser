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

/**
 * Apply a sparse settings patch to a settings object, mirroring the Rust
 * `UserSettings::with_patch`: defined fields replace wholesale, except
 * `keybindings` which merges (an empty accelerator resets that action).
 */
function applySettingsPatch(
  settings: UserSettings,
  patch: UserSettingsPatch,
): UserSettings {
  const { keybindings, ...rest } = patch
  const defined = Object.fromEntries(
    Object.entries(rest).filter(([, value]) => value !== undefined),
  ) as Partial<UserSettings>
  const next: UserSettings = { ...settings, ...defined }
  if (keybindings) {
    const merged: Record<string, string> = { ...settings.keybindings }
    for (const [action, accel] of Object.entries(keybindings)) {
      if (accel === '') delete merged[action]
      else merged[action] = accel
    }
    next.keybindings = merged as UserSettings['keybindings']
  }
  return next
}

export function useUpdateSettings() {
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (patch: UserSettingsPatch) => commands.updateSettings(patch),
    onMutate: (patch: UserSettingsPatch) => {
      qc.setQueryData<BrowserState>([...BROWSER_STATE_KEY], (old) =>
        old ? { ...old, settings: applySettingsPatch(old.settings, patch) } : old,
      )
    },
    onSettled: () => {
      qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY })
    },
  })
}
