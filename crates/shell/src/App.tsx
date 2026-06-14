import { Window } from '@/components/Window'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { PendingRenameProvider } from '@/hooks/usePendingRename'
import { useSessionRestore } from '@/hooks/useSessionRestore'
import { useThemeApply } from '@/hooks/useThemeApply'

export default function App() {
  const { data: state, isLoading, error } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)
  useSessionRestore()

  if (isLoading || !state) {
    return null
  }

  if (error) {
    return (
      <div
        style={{
          padding: 40,
          color: 'var(--destructive)',
          fontFamily: 'DM Mono, monospace',
          fontSize: 12,
        }}
      >
        Failed to load browser state: {(error as Error).message}
      </div>
    )
  }

  return (
    <PendingRenameProvider>
      <Window state={state} settings={state.settings} />
    </PendingRenameProvider>
  )
}
