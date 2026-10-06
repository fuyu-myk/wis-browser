/**
 * The `about:source` page: the document a tab was showing when View Source ran.
 *
 * What it shows is the serialized live DOM, not the bytes the server sent. The
 * capture is held in Rust for this tab and is dropped when the tab closes.
 * Restored tabs from a previous session have no capture to show.
 */

import { useEffect, useState } from 'react'

import { commands } from '@/bindings/commands'
import type { PageSource, TabId } from '@/bindings/types'
import { showToast } from '@/lib/toast'

interface SourcePageProps {
  /** The tab the capture was taken from. */
  from: TabId
  target: string
}

export function SourcePage({ from, target }: SourcePageProps) {
  const [source, setSource] = useState<PageSource | null | undefined>(undefined)

  useEffect(() => {
    let cancelled = false
    void commands.pageSource(from).then(
      (found) => {
        if (!cancelled) setSource(found)
      },
      () => {
        if (!cancelled) setSource(null)
      },
    )
    return () => {
      cancelled = true
    }
  }, [from])

  if (source === undefined) return <div className="kb-source-page" />

  return (
    <div className="kb-source-page">
      <header className="kb-source-head">
        <span className="kb-source-target" title={target}>
          {target}
        </span>
        <span className="kb-source-note">
          The page as it is now, not the file the server sent.
          {source?.truncated ? ' Too large to show in full.' : ''}
        </span>
        {source && (
          <button
            type="button"
            className="kb-btn"
            onClick={() => {
              void commands
                .copyText(source.chunks.join(''))
                .then(() => showToast('Source copied'))
                .catch(() => showToast('Copying the source failed'))
            }}
          >
            Copy
          </button>
        )}
      </header>
      {source ? (
        <div className="kb-source-body">
          {source.chunks.map((chunk, index) => (
            <pre className="kb-source-chunk" key={index}>
              {chunk}
            </pre>
          ))}
        </div>
      ) : (
        <p className="kb-source-empty">
          This source is no longer held. Captures live only as long as the tab showing them, so one
          reopened from a previous session was already dropped. View Source again from the page.
        </p>
      )}
    </div>
  )
}
