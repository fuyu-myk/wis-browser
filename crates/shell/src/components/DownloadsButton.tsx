import { useEffect, useRef, useState } from 'react'

import type { DownloadId, DownloadStatus } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { DownloadIcon } from '@/lib/icons'
import { DOWNLOADS_OPEN_EVENT } from '@/lib/downloads'
import { useDownloads } from '@/hooks/useDownloads'

/** Opens the downloads popup (palette-style overlay) via a DOM event. A badge
 * reports download state: a spinner while one runs, then a transient green
 * check on completion or amber exclamation on failure. Rendered as a circle
 * button in the sidebar footer and the top-tabs rail controls. */
export function DownloadsButton() {
  const { downloads, active, ready } = useDownloads()
  const className = cx('kb-circle-btn', 'kb-rail-dl')

  // Status transitions flash a verdict: a download that was pending (or so
  // fast it appears already finished between refetches) shows its outcome
  // briefly; failure wins over success in the same batch. The baseline is
  // the first loaded snapshot, so persisted history never flashes at start.
  const [flash, setFlash] = useState<'done' | 'failed' | null>(null)
  const prevRef = useRef<Map<DownloadId, DownloadStatus> | null>(null)
  useEffect(() => {
    if (!ready) return
    const prev = prevRef.current
    prevRef.current = new Map(downloads.map((d) => [d.id, d.status]))
    if (prev === null) return
    let done = false
    let failed = false
    for (const d of downloads) {
      const p = prev.get(d.id)
      const wasPending = p === 'in_progress' || p === undefined
      if (!wasPending || p === d.status) continue
      if (d.status === 'completed') done = true
      else if (d.status === 'failed') failed = true
    }
    if (failed) setFlash('failed')
    else if (done) setFlash('done')
  }, [downloads, ready])
  useEffect(() => {
    if (flash === null) return
    const t = window.setTimeout(() => setFlash(null), 3200)
    return () => window.clearTimeout(t)
  }, [flash])

  const badge = active ? 'active' : flash

  return (
    <button
      type="button"
      className={className}
      aria-label="Downloads"
      title="Downloads"
      onClick={() => window.dispatchEvent(new CustomEvent(DOWNLOADS_OPEN_EVENT))}
    >
      <DownloadIcon />
      {badge === 'active' && <span className="kb-dl-badge is-active" aria-hidden />}
      {badge === 'done' && (
        <span className="kb-dl-badge is-done" aria-hidden>
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="4"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M20 6 9 17l-5-5" />
          </svg>
        </span>
      )}
      {badge === 'failed' && (
        <span className="kb-dl-badge is-failed" aria-hidden>
          !
        </span>
      )}
    </button>
  )
}
