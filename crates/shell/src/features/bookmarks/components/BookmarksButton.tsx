import { cx } from '@/lib/cx'
import { BOOKMARKS_OPEN_EVENT } from '@/features/bookmarks/lib/bookmarkMenu'

/** Opens the bookmarks popup (palette-style overlay) via a DOM event.
 * Rendered as a circle button in the sidebar footer and the top-tabs rail. */
export function BookmarksButton() {
  return (
    <button
      type="button"
      className={cx('kb-circle-btn', 'kb-rail-bm')}
      aria-label="Bookmarks"
      title="Bookmarks"
      onClick={() => window.dispatchEvent(new CustomEvent(BOOKMARKS_OPEN_EVENT))}
    >
      <BookmarkGlyph />
    </button>
  )
}

function BookmarkGlyph() {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="M6 3h12a1 1 0 0 1 1 1v17l-7-4-7 4V4a1 1 0 0 1 1-1Z" />
    </svg>
  )
}
