import type { LinkRoute, WorkspaceId } from '@/bindings/types'

export const MAX_LINK_ROUTES = 200
const SITE = /^(?:[\p{L}\p{N}_-]+\.)*[\p{L}\p{N}_-]+\.?$/u

/** Wis checks the site again when it saves (`wis_core::link_routing`). */
function tidied(input: string): string {
  return input
    .trim()
    .toLowerCase()
    .replace(/^https?:\/\//, '')
    .replace(/^\*?\./, '')
    .replace(/^www\./, '')
    .replace(/[?#].*$/, '')
    .replace(/\/+$/, '')
}

export function looksLikeSite(input: string): boolean {
  const site = tidied(input).split('/')[0] ?? ''
  return (
    !/\s/.test(input.trim()) && SITE.test(site) && (site.includes('.') || site === 'localhost')
  )
}

export function withRoute(
  routes: ReadonlyArray<LinkRoute>,
  input: string,
  workspace: WorkspaceId,
): LinkRoute[] {
  const pattern = tidied(input)
  return [...routes.filter((route) => route.pattern !== pattern), { pattern, workspace_id: workspace }]
}

export function withoutRoute(routes: ReadonlyArray<LinkRoute>, pattern: string): LinkRoute[] {
  return routes.filter((route) => route.pattern !== pattern)
}

export function withImportedRoutes(
  current: ReadonlyArray<LinkRoute>,
  imported: ReadonlyArray<LinkRoute>,
): LinkRoute[] {
  const replaced = new Set(imported.map((route) => route.pattern))
  return [...current.filter((route) => !replaced.has(route.pattern)), ...imported].slice(
    0,
    MAX_LINK_ROUTES,
  )
}
