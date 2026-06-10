import { cx } from '@/lib/cx'
import { DownloadIcon } from '@/lib/icons'
import { DOWNLOADS_OPEN_EVENT } from '@/lib/downloads'
import { useDownloads } from '@/hooks/useDownloads'

interface DownloadsButtonProps {
  /** `fixed` = floating bubble in the sidebar's bottom stack;
   *  `rail` = inline circle button in the top-tabs rail controls. */
  variant: 'fixed' | 'rail'
  /** Edge the fixed bubble hugs (only for `fixed`). */
  anchor?: 'left' | 'right'
}

/** Opens the downloads popup (palette-style overlay) via a DOM event. A small
 * pulsing dot marks an in-flight download. */
export function DownloadsButton({ variant, anchor = 'left' }: DownloadsButtonProps) {
  const { active } = useDownloads()
  const className =
    variant === 'rail'
      ? cx('kb-circle-btn', 'kb-rail-dl')
      : cx('kb-dl-fixed', `is-${anchor}`)

  return (
    <button
      type="button"
      className={className}
      aria-label="Downloads"
      title="Downloads"
      onClick={() => window.dispatchEvent(new CustomEvent(DOWNLOADS_OPEN_EVENT))}
    >
      <DownloadIcon />
      {active ? <span className="kb-dl-active-dot" aria-hidden /> : null}
    </button>
  )
}
