import type { ShortcutTile } from '@/bindings/types'

/**
 * Reorder `tiles` to match `orderedUrls` (a tile's URL is its stable key, since
 * the list is deduped by URL at the boundary). Tiles whose URL is absent from
 * the order are dropped and unknown URLs ignored, so the result always maps 1:1
 * onto existing tiles.
 */
export function reorderByKeys(tiles: ShortcutTile[], orderedUrls: string[]): ShortcutTile[] {
  const byUrl = new Map(tiles.map((tile) => [tile.url, tile]))
  return orderedUrls
    .map((url) => byUrl.get(url))
    .filter((tile): tile is ShortcutTile => tile !== undefined)
}
