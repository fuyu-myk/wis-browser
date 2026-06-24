import { memo, useEffect, useState } from 'react'

import { useActivePrivacyMode } from '@/hooks/useActivePrivacyMode'
import { useWindowIconSwap } from '@/hooks/useWindowIcon'
import { WisMark } from '@/components/WisMark'

/**
 * The macOS traffic lights are rendered by the OS (Tauri uses
 * `titleBarStyle: "Overlay"`). The chrome only needs to reserve room for
 * them via CSS padding: see `.kb-topbar` / `.kb-side` rules. Kept as a
 * no-op export so existing call sites stay valid until they're migrated.
 */
export function TrafficLights() {
  return null
}

/**
 * Owl-only brand mark for sidebar and rail chrome surfaces. Strict workspaces
 * close the eyes as a steady signal that the browser is not watching.
 *
 * Memoized: it takes no props and now reads privacy mode via a narrowed
 * selector, so the parent's per-state-change re-render no longer cascades into
 * it. It re-renders only on hover or an actual privacy-mode change.
 */
export const Brand = memo(function Brand() {
  const [hovered, setHovered] = useState(false)
  const swapIcon = useWindowIconSwap()
  const mode = useActivePrivacyMode()
  const strict = mode === 'strict'

  // Workspace mode is the steady state for the dock/window icon; hover only
  // produces a transient flip and the steady state re-asserts on leave.
  useEffect(() => {
    swapIcon(strict ? 'closed' : 'open')
  }, [strict, swapIcon])

  const closed = strict || hovered

  return (
    <span
      className="kb-brand"
      aria-label="wis"
      onMouseEnter={() => {
        setHovered(true)
        swapIcon('closed')
      }}
      onMouseLeave={() => {
        setHovered(false)
        swapIcon(strict ? 'closed' : 'open')
      }}
    >
      <WisMark variant={closed ? 'solid-closed' : 'solid-open'} size={20} />
    </span>
  )
})
