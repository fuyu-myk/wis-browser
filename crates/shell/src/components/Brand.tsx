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
 * Owl-only brand mark for sidebar and rail chrome surfaces.
 */
export function Brand() {
  return (
    <span className="kb-brand" aria-label="wis">
      <WisMark variant="solid-open" size={20} />
    </span>
  )
}
