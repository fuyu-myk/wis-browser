import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { commands } from '@/bindings/commands'
import type { ImportSource, WorkspaceId } from '@/bindings/types'
import { Switch } from '@/components/controls'
import { cx } from '@/lib/cx'
import { errorMessage } from '@/lib/errors'
import { bookmarkImportSummary, historyImportSummary } from '@/lib/importSummary'

interface Status {
  kind: 'ok' | 'err'
  text: string
}

interface Kinds {
  bookmarks: boolean
  history: boolean
}

interface ImportSectionProps {
  workspaceId: WorkspaceId | null
  ephemeral: boolean
}

function sourceLabel(source: ImportSource, all: ReadonlyArray<ImportSource>): string {
  const profiles = all.filter((s) => s.browser === source.browser).length
  return profiles > 1 ? `${source.browser} (${source.profile})` : source.browser
}

function availableKinds(source: ImportSource): string {
  if (source.bookmarks && source.history) return 'Bookmarks and history'
  return source.bookmarks ? 'Bookmarks' : 'History'
}

export function ImportSection({ workspaceId, ephemeral }: ImportSectionProps) {
  const [kinds, setKinds] = useState<Kinds>({ bookmarks: true, history: true })
  const [busyId, setBusyId] = useState<string | null>(null)
  const [status, setStatus] = useState<Status[]>([])
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

  const wanted = (source: ImportSource): Kinds => ({
    bookmarks: kinds.bookmarks && source.bookmarks,
    history: kinds.history && source.history,
  })

  const importFrom = async (source: ImportSource) => {
    if (!workspaceId) return
    const label = sourceLabel(source, sources)
    const take = wanted(source)
    const lines: Status[] = []
    setStatus([])
    setBusyId(source.id)
    try {
      if (take.bookmarks) {
        try {
          const stats = await commands.importBrowserBookmarks(workspaceId, source.id)
          lines.push({
            kind: 'ok',
            text: bookmarkImportSummary(stats, `No bookmarks found in ${label}.`),
          })
        } catch (err: unknown) {
          lines.push({ kind: 'err', text: `Bookmarks: ${errorMessage(err)}` })
        }
      }
      if (take.history) {
        try {
          const stats = await commands.importBrowserHistory(workspaceId, source.id)
          lines.push({
            kind: 'ok',
            text: historyImportSummary(stats, `No recent history found in ${label}.`),
          })
        } catch (err: unknown) {
          lines.push({ kind: 'err', text: `History: ${errorMessage(err)}` })
        }
      }
    } finally {
      setStatus(lines)
      setBusyId(null)
    }
  }

  const intro = ephemeral
    ? 'Importing is disabled in private workspaces.'
    : !isLoading && !error && sources.length === 0
      ? 'No other browsers were found on this device. You can still import an HTML ' +
        'export under Bookmarks.'
      : 'Copy what you choose below into this workspace.'

  return (
    <section id="import" className="sec">
      <div className="sec-h">Import</div>
      <div className="row">
        <div className="l">
          <span>Import from another browser</span>
          <span>{intro}</span>
        </div>
      </div>
      {!ephemeral && (
        <>
          <div className="row">
            <div className="l">
              <span>Bookmarks</span>
              <span>Nested folders are combined into one level; duplicates are skipped.</span>
            </div>
            <Switch
              checked={kinds.bookmarks}
              onChange={(bookmarks) => setKinds({ ...kinds, bookmarks })}
              label="Import bookmarks"
            />
          </div>
          <div className="row">
            <div className="l">
              <span>History</span>
              <span>
                The last 180 days. Pages already in this workspace keep their own title.
              </span>
            </div>
            <Switch
              checked={kinds.history}
              onChange={(history) => setKinds({ ...kinds, history })}
              label="Import history"
            />
          </div>
          {sources.map((source) => {
            const take = wanted(source)
            return (
              <div className="row" key={source.id}>
                <div className="l">
                  <span>{sourceLabel(source, sources)}</span>
                  <span>{availableKinds(source)}</span>
                </div>
                <button
                  type="button"
                  className="kb-btn"
                  onClick={() => void importFrom(source)}
                  disabled={
                    busyId !== null || !workspaceId || !(take.bookmarks || take.history)
                  }
                  aria-label={`Import from ${sourceLabel(source, sources)}`}
                >
                  {busyId === source.id ? 'Importing…' : 'Import'}
                </button>
              </div>
            )
          })}
        </>
      )}
      {error && !ephemeral && (
        <p className="kb-backup-status is-error">{errorMessage(error)}</p>
      )}
      {status.map((line) => (
        <p
          key={line.text}
          className={cx('kb-backup-status', { 'is-error': line.kind === 'err' })}
        >
          {line.text}
        </p>
      ))}
    </section>
  )
}
