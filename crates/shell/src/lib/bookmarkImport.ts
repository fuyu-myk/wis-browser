import type { BookmarkImportStats } from '@/bindings/types'

function plural(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? '' : 's'}`
}

export function bookmarkImportSummary(stats: BookmarkImportStats, emptyText: string): string {
  if (stats.bookmarks_added === 0 && stats.folders_added === 0) {
    return stats.bookmarks_skipped > 0
      ? `Nothing new: ${plural(stats.bookmarks_skipped, 'bookmark')} already saved.`
      : emptyText
  }
  const bits = [plural(stats.bookmarks_added, 'bookmark')]
  if (stats.folders_added > 0) bits.push(plural(stats.folders_added, 'folder'))
  const skipped =
    stats.bookmarks_skipped > 0 ? ` (${plural(stats.bookmarks_skipped, 'duplicate')} skipped)` : ''
  return `Imported ${bits.join(' and ')}${skipped}.`
}
