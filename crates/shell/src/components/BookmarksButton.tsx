import { cx } from '@/lib/cx'
import { BOOKMARKS_OPEN_EVENT } from '@/lib/bookmarkMenu'

interface BookmarksButtonProps {
  /** `fixed` = floating bubble above the sidebar VPN/settings cogs;
   *  `rail` = inline circle button in the top-tabs rail controls. */
  variant: 'fixed' | 'rail'
  /** Edge the fixed bubble hugs (only for `fixed`). */
  anchor?: 'left' | 'right'
}

/** Opens the bookmarks popup (palette-style overlay) via a DOM event. */
export function BookmarksButton({ variant, anchor = 'left' }: BookmarksButtonProps) {
  const className =
    variant === 'rail'
      ? cx('kb-circle-btn', 'kb-rail-bm')
      : cx('kb-bm-fixed', `is-${anchor}`)

  return (
    <button
      type="button"
      className={className}
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
