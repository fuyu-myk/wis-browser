import type { UserSettings } from '@/bindings/types'

/**
 * Whether floating menus / popovers (overlay surfaces) should render glass:
 * the master glass toggle on and the "menus" scope enabled. Defaults to true
 * while settings are still loading, matching the chrome's optimistic default.
 *
 * Overlay surfaces are separate webviews that attach this as an element class
 * (`glass-menus`), since their menu element is the surface root rather than a
 * descendant of the chrome body.
 */
export function menusGlass(settings: UserSettings | undefined): boolean {
  return (settings?.glass_enabled ?? true) && (settings?.design.glass_scopes.menus ?? true)
}
