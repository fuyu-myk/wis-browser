import type { Tab, TabFolder, TabFolderId, TabId } from '@/bindings/types'

/* Phase boundary: active tab can't shrink below this (in px). */
export const MINIMUM_CURRENT_TAB_SIZE = 260
export const COMPACT_MINIMUM_CURRENT_TAB_SIZE = 180
/* Final floor: inactive tabs can't shrink below this (favicon visible). */
export const MINIMUM_TAB_SIZE = 44
export const COMPACT_MINIMUM_TAB_SIZE = 40
/* Cap on expanded-island members: filing a tab compacts it into the pill
   instead of keeping its loose share (loose tabs absorb the slack via
   flex-grow). The active member caps at the active floor instead, so its
   title stays readable. */
export const MEMBER_TAB_MAX = 150
export const COMPACT_MEMBER_TAB_MAX = 120
export const GAP_PX = 4
/* Fixed widths reserved by islands before sizing the flexible tabs. */
export const ISLAND_CIRCLE_W = 30
export const ISLAND_NAME_W = 96
export const ISLAND_NAME_W_COMPACT = 74
export const ISLAND_NAME_MAX = 168
export const ISLAND_NAME_MAX_COMPACT = 150
export const ISLAND_PILL_PAD = 3
export const PIN_CIRCLE_W = 30
/* Speaker button on audible/muted tabs (never shrinks), plus the in-tab
   flex gap it introduces: reserved per audio tab so the minimum sizes
   still fit favicon + speaker. */
export const AUDIO_BTN_W = 16
export const TAB_CONTENT_GAP = 8
export const COMPACT_TAB_CONTENT_GAP = 6
export const ISLAND_EXIT_MS = 200

export interface SizingResult {
  activeSize: number
  inactiveSize: number
  /** True when total content exceeds the strip and scrolling is needed. */
  scrolling: boolean
}

export interface FolderGroup {
  folder: TabFolder
  members: Tab[]
}

/* Chip label measurement (cached): the name chip fits its text up to a cap
   instead of truncating at the fixed minimum while the strip sits empty. */
let chipMeasureCtx: CanvasRenderingContext2D | null = null
const chipTextWidths = new Map<string, number>()
export function chipTextWidth(name: string): number {
  const cached = chipTextWidths.get(name)
  if (cached !== undefined) return cached
  if (chipMeasureCtx === null) {
    chipMeasureCtx = document.createElement('canvas').getContext('2d')
    if (chipMeasureCtx) {
      chipMeasureCtx.font =
        "600 12px Geist, 'DM Sans', -apple-system, BlinkMacSystemFont, sans-serif"
    }
  }
  const width = chipMeasureCtx ? Math.ceil(chipMeasureCtx.measureText(name).width) : 0
  chipTextWidths.set(name, width)
  return width
}

/* Chip box: 5px left pad + 15px chevron + 3px gap + optional icon (11px
   glyph + 4px label gap) + text + 9px right pad, clamped between the fixed
   minimum and the cap. */
export function chipWidthFor(folder: TabFolder, compact: boolean): number {
  const nameW = compact ? ISLAND_NAME_W_COMPACT : ISLAND_NAME_W
  const fixed = 5 + 15 + 3 + 9 + (folder.icon ? 11 + 4 : 0)
  const fit = fixed + chipTextWidth(folder.name)
  const max = compact ? ISLAND_NAME_MAX_COMPACT : ISLAND_NAME_MAX
  return Math.min(Math.max(nameW, fit), max)
}

/**
 * Distribute `budget` (already net of all gaps and the islands' fixed parts)
 * across `n` flexible tabs. The active tab keeps `minActive` when space runs
 * short; below the inactive floor the strip scrolls.
 */
export function computeSizing(
  n: number,
  budget: number,
  minActive: number,
  minInactive: number,
): SizingResult {
  if (n <= 0 || budget <= 0) {
    return { activeSize: minActive, inactiveSize: minInactive, scrolling: n > 0 }
  }

  const equalSize = budget / n
  if (equalSize >= minActive) {
    return { activeSize: equalSize, inactiveSize: equalSize, scrolling: false }
  }

  if (n === 1) {
    return { activeSize: minActive, inactiveSize: minInactive, scrolling: budget < minActive }
  }
  const calcInactiveSize = (budget - minActive) / (n - 1)
  if (calcInactiveSize >= minInactive) {
    return { activeSize: minActive, inactiveSize: calcInactiveSize, scrolling: false }
  }

  return { activeSize: minActive, inactiveSize: minInactive, scrolling: true }
}

/** Move `id` to `insertion` within `order`; null when nothing changes. */
export function moveInList<T>(order: T[], id: T, insertion: number): T[] | null {
  const from = order.indexOf(id)
  if (from < 0) return null
  const without = order.filter((x) => x !== id)
  const at = insertion > from ? insertion - 1 : insertion
  without.splice(at, 0, id)
  if (without.length === order.length && without.every((x, i) => x === order[i])) return null
  return without
}

export interface StripSizingArgs {
  sizableTabs: Tab[]
  looseTabs: Tab[]
  pinnedCount: number
  folderGroups: FolderGroup[]
  collapsedCount: number
  closingIslandId: TabFolderId | null
  stripWidth: number
  compact: boolean
  activeTabId: TabId | null
  minActive: number
  minInactive: number
  isAudio: (tab: Tab) => boolean
}

export interface StripSizing {
  sizing: SizingResult
  isMany: boolean
  inactiveCompact: boolean
  /** Extra width a tab reserves for its audio speaker button (+ its gap). */
  audioExtra: number
  /** Whether the active tab is among the flexible (sizable) tabs. */
  activeVisible: boolean
}

/**
 * Reserve the pinned circles, islands' fixed parts (circles, name chips +
 * pill padding), every gap, and the audio tabs' speaker buttons, then size
 * the flexible tabs to fill exactly what's left.
 */
export function computeStripSizing(a: StripSizingArgs): StripSizing {
  const n = a.sizableTabs.length
  const activeVisible = a.sizableTabs.some((t) => t.id === a.activeTabId)
  const audioExtra = AUDIO_BTN_W + (a.compact ? COMPACT_TAB_CONTENT_GAP : TAB_CONTENT_GAP)
  const audioCount = a.sizableTabs.reduce((c, t) => c + (a.isAudio(t) ? 1 : 0), 0)
  const expandedMembers = n - a.looseTabs.length
  const topLevelCount = a.pinnedCount + a.folderGroups.length + a.looseTabs.length
  const totalGaps = Math.max(0, topLevelCount - 1) * GAP_PX + expandedMembers * GAP_PX
  const expandedChipsWidth = a.folderGroups.reduce(
    (sum, g) =>
      g.folder.collapsed || g.folder.id === a.closingIslandId
        ? sum
        : sum + chipWidthFor(g.folder, a.compact) + ISLAND_PILL_PAD * 2,
    0,
  )
  const fixedWidth =
    a.pinnedCount * PIN_CIRCLE_W + a.collapsedCount * ISLAND_CIRCLE_W + expandedChipsWidth
  const available = Math.max(0, a.stripWidth - fixedWidth - totalGaps - audioCount * audioExtra)
  const sizing = computeSizing(n, available, a.minActive, a.minInactive)
  const isMany = sizing.activeSize !== sizing.inactiveSize
  const inactiveCompact = isMany && sizing.inactiveSize < (a.compact ? 90 : 130)
  return { sizing, isMany, inactiveCompact, audioExtra, activeVisible }
}
