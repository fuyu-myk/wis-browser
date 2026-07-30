import { useEffect, useRef, useState } from 'react'
import type { CSSProperties } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import type { DownloadId, DownloadItem, DownloadStatus } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { DownloadIcon } from '@/lib/icons'
import { DOWNLOADS_OPEN_EVENT, DOWNLOADS_SHOWN_EVENT } from '@/lib/downloads'
import { useDownloads } from '@/hooks/useDownloads'

/** Aggregate completion of the in-flight set, or null when any item lacks a
 * total (a mixed sum would lie) so the indeterminate spinner shows instead.
 * Clamped: Content-Length counts encoded bytes while the on-disk poll counts
 * decoded ones, so the ratio can overshoot. Completion never comes from the
 * pie, only from the status change. */
function aggregateFraction(inflight: DownloadItem[]): number | null {
  if (inflight.length === 0) return null
  let received = 0
  let total = 0
  for (const d of inflight) {
    if (d.totalBytes == null || d.totalBytes <= 0) return null
    received += d.receivedBytes
    total += d.totalBytes
  }
  return Math.min(1, received / total)
}

/** Opens the downloads popup (palette-style overlay) via a DOM event. A badge
 * reports download state: a theme-colored pie filling with progress (spinner
 * when the total is unknown), then a transient drawn-in check on completion
 * or amber exclamation on failure. After the check, the glyph stays tinted
 * until the downloads menu is opened. Rendered as a circle button in the
 * sidebar footer and the top-tabs rail controls. */
export function DownloadsButton() {
  const { downloads, active, ready } = useDownloads()

  // Status transitions flash a verdict: a download that was pending (or so
  // fast it appears already finished between refetches) shows its outcome
  // briefly; failure wins over success in the same batch. The baseline is
  // the first loaded snapshot, so persisted history never flashes at start.
  const [flash, setFlash] = useState<'done' | 'failed' | null>(null)
  // Completed downloads the user has not looked at yet (cleared when the
  // downloads popup is shown). Session-local by design.
  const [unseen, setUnseen] = useState(false)
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
    else if (done) {
      setFlash('done')
      setUnseen(true)
    }
  }, [downloads, ready])
  useEffect(() => {
    if (flash === null) return
    const t = window.setTimeout(() => setFlash(null), 3200)
    return () => window.clearTimeout(t)
  }, [flash])

  useEffect(() => {
    let cancelled = false
    let unlisten: UnlistenFn | null = null
    void listen<void>(DOWNLOADS_SHOWN_EVENT, () => setUnseen(false)).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  const badge = active ? 'active' : flash
  const frac = active ? aggregateFraction(downloads.filter((d) => d.status === 'in_progress')) : null

  return (
    <button
      type="button"
      className={cx('kb-circle-btn', 'kb-rail-dl', {
        'is-unseen': unseen && badge === null,
      })}
      aria-label="Downloads"
      title="Downloads"
      onClick={() => window.dispatchEvent(new CustomEvent(DOWNLOADS_OPEN_EVENT))}
    >
      <DownloadIcon />
      {badge === 'active' &&
        (frac !== null ? (
          <span
            className="kb-dl-badge is-pie"
            style={{ '--dl-frac': frac } as CSSProperties}
            aria-hidden
          />
        ) : (
          <span className="kb-dl-badge is-active" aria-hidden />
        ))}
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
