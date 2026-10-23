/**
 * Page-zoom readout, sitting inside the address bar beside its other trailing
 * controls and matching their height. It is absent while the page sits at the
 * default zoom.
 *
 * Each character is keyed by its distance from the end plus its own value, so
 * React remounts exactly the characters that changed and leaves the rest
 * alone.
 *
 * A remounted character replays one of two entrances: a larger zoom rises
 * from below, a smaller one drops from above, matching the direction the page
 * itself just moved.
 */

import { useEffect, useRef } from 'react'

import type { Tab } from '@/bindings/types'
import { useBrowserState } from '@/hooks/useBrowserState'
import { cx } from '@/lib/cx'
import { hostOfUrl, zoomPercentForHost } from '@/features/zoom/zoom'

export function ZoomIndicator({ activeTab }: { activeTab: Tab | null }) {
  const { data: state } = useBrowserState()
  const settings = state?.settings
  const defaultPercent = settings?.default_zoom_percent ?? 100
  const percent =
    activeTab && settings
      ? zoomPercentForHost(hostOfUrl(activeTab.url), settings.zoom_site_overrides, defaultPercent)
      : defaultPercent

  // Read before the effect writes it, so this render sees the previous value.
  const previous = useRef(percent)
  const falling = percent < previous.current
  useEffect(() => {
    previous.current = percent
  }, [percent])

  const shown = percent !== defaultPercent
  const text = `${percent}%`

  return (
    <span
      className={cx('kb-zoom-chip', { 'is-on': shown })}
      role="status"
      aria-hidden={!shown}
      title={`Page zoom ${percent}%`}
    >
      {[...text].map((char, index) => (
        <ZoomChar key={`${text.length - 1 - index}:${char}`} char={char} falling={falling} />
      ))}
    </span>
  )
}

/**
 * One character of the readout. The direction is captured at mount and never
 * updated.
 */
function ZoomChar({ char, falling }: { char: string; falling: boolean }) {
  const direction = useRef(falling)
  return (
    <span className={cx('kb-zoom-char', { 'is-falling': direction.current })}>{char}</span>
  )
}
