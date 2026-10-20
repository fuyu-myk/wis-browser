import type { SiteProtection, TabId } from '@/bindings/types'
import type { Box } from '@/lib/extensionsMenu'
import { normalizeHostInput } from '@/lib/host'

export const SITE_PANEL_OPEN = 'wis-sitepanel-open'
export const SITE_PANEL_READY = 'wis-sitepanel-ready'
export const SITE_PANEL_DONE = 'wis-sitepanel-done'
export const SITE_PANEL_SIZE = 'wis-sitepanel-size'
export const SITE_PROTECTION_SESSION_EVENT = 'wis-site-protection-session'

export const PANEL_WIDTH = 280
export const UNMEASURED_PANEL_HEIGHT = 140
const GAP = 6
const MARGIN = 8

export interface SitePanelPayload {
  window: string
  tabId: TabId
  url: string
}

const PROTECTED: SiteProtection = { blocking: true, javascript: true }

/** Below `anchor` with the left edges lined up, above it when there is no
 *  room below, and always inside the window. */
export function panelBounds(anchor: Box, height: number, viewport: { width: number; height: number }): Box {
  const x = Math.min(Math.max(MARGIN, anchor.x), viewport.width - PANEL_WIDTH - MARGIN)
  const under = anchor.y + anchor.height + GAP
  const y = under + height > viewport.height - MARGIN ? Math.max(MARGIN, anchor.y - GAP - height) : under
  return { x, y, width: PANEL_WIDTH, height }
}

export function siteOf(url: string): string | null {
  try {
    const { protocol } = new URL(url)
    if (protocol !== 'http:' && protocol !== 'https:') return null
  } catch {
    return null
  }
  return normalizeHostInput(url) || null
}

type Overrides = Record<string, SiteProtection> | undefined

/** `session` holds the switches changed in private workspaces. */
export function siteProtection(saved: Overrides, session: Overrides, site: string): SiteProtection {
  return session?.[site] ?? saved?.[site] ?? PROTECTED
}

export function isUnprotected(protection: SiteProtection): boolean {
  return !protection.blocking || !protection.javascript
}
