import { useState } from 'react'

import { commands } from '@/bindings/commands'
import type { WorkspaceId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { bookmarkImportSummary } from '@/lib/importSummary'

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
      setStatus({
        kind: 'ok',
        text: bookmarkImportSummary(stats, 'No bookmarks found in that file.'),
      })
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
