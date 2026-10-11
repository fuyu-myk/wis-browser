import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { commands } from '@/bindings/commands'
import type { ImportSource, WorkspaceId } from '@/bindings/types'
import { bookmarkImportSummary } from '@/lib/bookmarkImport'
import { cx } from '@/lib/cx'
import { errorMessage } from '@/lib/errors'

interface Status {
  kind: 'ok' | 'err'
  text: string
}

interface ImportSectionProps {
  workspaceId: WorkspaceId | null
  ephemeral: boolean
}

function sourceLabel(source: ImportSource, all: ReadonlyArray<ImportSource>): string {
  const profiles = all.filter((s) => s.browser === source.browser).length
  return profiles > 1 ? `${source.browser} (${source.profile})` : source.browser
}

export function ImportSection({ workspaceId, ephemeral }: ImportSectionProps) {
  const [busyId, setBusyId] = useState<string | null>(null)
  const [status, setStatus] = useState<Status | null>(null)
  const {
    data: sources = [],
    isLoading,
    error,
  } = useQuery({
    queryKey: ['import-sources'],
    queryFn: () => commands.listImportSources(),
    enabled: !ephemeral,
    staleTime: 0,
  })

  const importFrom = async (source: ImportSource) => {
    if (!workspaceId) return
    setStatus(null)
    setBusyId(source.id)
    try {
      const stats = await commands.importBrowserBookmarks(workspaceId, source.id)
      const empty = `No bookmarks found in ${sourceLabel(source, sources)}.`
      setStatus({ kind: 'ok', text: bookmarkImportSummary(stats, empty) })
    } catch (err: unknown) {
      setStatus({ kind: 'err', text: errorMessage(err) })
    } finally {
      setBusyId(null)
    }
  }

  const intro = ephemeral
    ? 'Importing is disabled in private workspaces.'
    : !isLoading && !error && sources.length === 0
      ? 'No other browsers were found on this device. You can still import an HTML ' +
        'export under Bookmarks.'
      : 'Copy bookmarks into this workspace. Nested folders are combined into one ' +
        'level; duplicates are skipped.'

  return (
    <section id="import" className="sec">
      <div className="sec-h">Import</div>
      <div className="row">
        <div className="l">
          <span>Import from another browser</span>
          <span>{intro}</span>
        </div>
      </div>
      {!ephemeral &&
        sources.map((source) => (
          <div className="row" key={source.id}>
            <div className="l">
              <span>{sourceLabel(source, sources)}</span>
              <span>Bookmarks</span>
            </div>
            <button
              type="button"
              className="kb-btn"
              onClick={() => void importFrom(source)}
              disabled={busyId !== null || !workspaceId}
              aria-label={`Import bookmarks from ${sourceLabel(source, sources)}`}
            >
              {busyId === source.id ? 'Importing…' : 'Import'}
            </button>
          </div>
        ))}
      {error && !ephemeral && (
        <p className="kb-backup-status is-error">{errorMessage(error)}</p>
      )}
      {status && (
        <p className={cx('kb-backup-status', { 'is-error': status.kind === 'err' })}>
          {status.text}
        </p>
      )}
    </section>
  )
}
