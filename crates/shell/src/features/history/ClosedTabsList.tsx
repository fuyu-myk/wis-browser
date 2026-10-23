import type { ClosedTabRecord } from '@/bindings/types'
import { type DayGroup, formatTime, hostOf } from '@/features/history/dayGroups'
import { XIcon } from '@/lib/icons'

interface ClosedTabsListProps {
  groups: DayGroup<ClosedTabRecord>[]
  /** Workspace names, shown in the all-workspaces view. */
  workspaceNames?: Map<string, string>
  onReopen: (id: number) => void
  onForget: (id: number) => void
  attachScroll: (el: HTMLElement | null) => void
}

export function ClosedTabsList({
  groups,
  workspaceNames,
  onReopen,
  onForget,
  attachScroll,
}: ClosedTabsListProps) {
  return (
    <div className="kb-history-groups" ref={attachScroll}>
      {groups.map((group) => (
        <section key={group.key} className="kb-history-group">
          <h2 className="kb-history-day">{group.label}</h2>
          <ul>
            {group.entries.map((entry) => (
              <ClosedTabRow
                key={entry.id}
                entry={entry}
                workspaceLabel={workspaceNames?.get(entry.workspace_id)}
                onReopen={() => onReopen(entry.id)}
                onForget={() => onForget(entry.id)}
              />
            ))}
          </ul>
        </section>
      ))}
    </div>
  )
}

interface ClosedTabRowProps {
  entry: ClosedTabRecord
  workspaceLabel?: string
  onReopen: () => void
  onForget: () => void
}

function ClosedTabRow({ entry, workspaceLabel, onReopen, onForget }: ClosedTabRowProps) {
  const host = hostOf(entry.url)
  const title = entry.title.trim() || host || entry.url
  return (
    <li className="kb-history-row">
      <button
        type="button"
        className="kb-history-link"
        data-menu-url={entry.url}
        data-menu-label={title}
        onClick={onReopen}
        title={`Reopen ${entry.url}`}
      >
        <span className="kb-history-time">{formatTime(entry.closed_at)}</span>
        <span className="kb-history-url">{host}</span>
        <span className="kb-history-title">{title}</span>
        {entry.archived ? (
          <span className="kb-closed-archived" title="Closed after sitting unused; listed by when it was last used">
            Archived
          </span>
        ) : null}
        {workspaceLabel ? <span className="kb-history-ws">{workspaceLabel}</span> : null}
      </button>
      <button
        type="button"
        className="kb-history-del"
        aria-label="Remove from recently closed"
        title="Remove from recently closed"
        onClick={onForget}
      >
        <XIcon />
      </button>
    </li>
  )
}
