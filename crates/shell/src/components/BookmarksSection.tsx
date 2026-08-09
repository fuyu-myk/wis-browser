import { useState } from 'react'

import { commands } from '@/bindings/commands'
import type { WorkspaceId } from '@/bindings/types'
import { cx } from '@/lib/cx'

interface Status {
  kind: 'ok' | 'err'
  text: string
}

function errorMessage(error: unknown): string {
  if (typeof error === 'string') return error
  if (error && typeof error === 'object' && 'message' in error) {
    const message = (error as { message?: unknown }).message
    if (typeof message === 'string') return message
  }
  return 'unknown error'
}

function plural(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? '' : 's'}`
}

interface BookmarksSectionProps {
  workspaceId: WorkspaceId | null
  ephemeral: boolean
}

export function BookmarksSection({ workspaceId, ephemeral }: BookmarksSectionProps) {
  const [busy, setBusy] = useState(false)
  const [status, setStatus] = useState<Status | null>(null)

  const importHtml = async () => {
    if (!workspaceId) return
    setStatus(null)
    setBusy(true)
    try {
      const stats = await commands.importBookmarksHtml(workspaceId)
      if (!stats) return
      if (stats.bookmarks_added === 0 && stats.folders_added === 0) {
        setStatus({
          kind: 'ok',
          text:
            stats.bookmarks_skipped > 0
              ? `Nothing new: ${plural(stats.bookmarks_skipped, 'bookmark')} already saved.`
              : 'No bookmarks found in that file.',
        })
        return
      }
      const bits = [plural(stats.bookmarks_added, 'bookmark')]
      if (stats.folders_added > 0) bits.push(plural(stats.folders_added, 'folder'))
      const skipped =
        stats.bookmarks_skipped > 0
          ? ` (${plural(stats.bookmarks_skipped, 'duplicate')} skipped)`
          : ''
      setStatus({ kind: 'ok', text: `Imported ${bits.join(' and ')}${skipped}.` })
    } catch (error: unknown) {
      setStatus({ kind: 'err', text: errorMessage(error) })
    } finally {
      setBusy(false)
    }
  }

  const exportHtml = async () => {
    if (!workspaceId) return
    setStatus(null)
    setBusy(true)
    try {
      const written = await commands.exportBookmarksHtml(workspaceId)
      if (written) setStatus({ kind: 'ok', text: 'Bookmarks exported.' })
    } catch (error: unknown) {
      setStatus({ kind: 'err', text: errorMessage(error) })
    } finally {
      setBusy(false)
    }
  }

  return (
    <section id="bookmarks" className="sec">
      <div className="sec-h">Bookmarks</div>
      <div className="row">
        <div className="l">
          <span>Import from HTML</span>
          <span>
            {ephemeral
              ? 'Bookmarks are disabled in private workspaces.'
              : 'Bring bookmarks from another browser: export them there as an HTML ' +
                'file, then pick it here. Nested folders are combined into one level; ' +
                'duplicates are skipped.'}
          </span>
        </div>
        <button
          type="button"
          className="kb-btn"
          onClick={() => void importHtml()}
          disabled={busy || ephemeral || !workspaceId}
        >
          Import…
        </button>
      </div>
      <div className="row">
        <div className="l">
          <span>Export to HTML</span>
          <span>Save this workspace&apos;s bookmarks as a file any browser can import.</span>
        </div>
        <button
          type="button"
          className="kb-btn"
          onClick={() => void exportHtml()}
          disabled={busy || !workspaceId}
        >
          Export…
        </button>
      </div>
      {status && (
        <p className={cx('kb-backup-status', { 'is-error': status.kind === 'err' })}>
          {status.text}
        </p>
      )}
    </section>
  )
}
