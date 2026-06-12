import type { TabId } from '@/bindings/types'
import { VolumeOffIcon, VolumeOnIcon } from '@/lib/icons'

interface TabAudioButtonProps {
  id: TabId
  audible: boolean
  muted: boolean
  onToggle: (id: TabId) => void
}

/**
 * Speaker beside the tab favicon while the tab plays audio (slashed while
 * muted, shown persistently then so it can unmute). Clicking toggles the
 * engine-level page mute without activating the tab; pointer events stop
 * here so the click neither focuses the tab nor starts a tab drag.
 */
export function TabAudioButton({ id, audible, muted, onToggle }: TabAudioButtonProps) {
  if (!audible && !muted) return null
  return (
    <button
      type="button"
      className="kb-tab-audio"
      title={muted ? 'Unmute tab' : 'Mute tab'}
      aria-label={muted ? 'Unmute tab' : 'Mute tab'}
      onPointerDown={(e) => e.stopPropagation()}
      onClick={(e) => {
        e.stopPropagation()
        onToggle(id)
      }}
    >
      {muted ? <VolumeOffIcon /> : <VolumeOnIcon />}
    </button>
  )
}
