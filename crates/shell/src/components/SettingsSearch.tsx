import { useMemo, useState, type KeyboardEvent } from 'react'

import { cx } from '@/lib/cx'
import { SearchIcon, XIcon } from '@/lib/icons'
import { searchSettings, type SettingEntry } from '@/lib/settingsSearch'

const LIST_ID = 'kb-settings-results'

interface FoundRow extends SettingEntry {
  row: HTMLElement
}

/** Every rendered settings row under `root`, named by its title, description and section. */
function collectRows(root: HTMLElement | null): FoundRow[] {
  if (!root) return []
  return Array.from(root.querySelectorAll<HTMLElement>('.sec .row')).flatMap((row) => {
    const spans = row.querySelectorAll(':scope > .l > span')
    if (spans.length === 0 || row.offsetParent === null) return []
    return [
      {
        row,
        section: row.closest('.sec')?.querySelector('.sec-h')?.textContent ?? '',
        title: spans[0].textContent ?? '',
        detail: spans.length > 1 ? (spans[spans.length - 1].textContent ?? '') : '',
      },
    ]
  })
}

interface SettingsSearchProps {
  query: string
  onQuery: (query: string) => void
  /** The scrolling column holding every section. */
  root: () => HTMLElement | null
  onPick: (row: HTMLElement) => void
}

/** A search field whose matches replace the section list while there is a query. */
export function SettingsSearch({ query, onQuery, root, onPick }: SettingsSearchProps) {
  const [active, setActive] = useState(0)
  const results = useMemo(() => {
    const rows = collectRows(root())
    return searchSettings(rows, query).map((index) => rows[index])
  }, [query, root])

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if ((e.key === 'ArrowDown' || e.key === 'ArrowUp') && results.length > 0) {
      e.preventDefault()
      const step = e.key === 'ArrowDown' ? 1 : -1
      setActive((i) => (i + step + results.length) % results.length)
    } else if (e.key === 'Enter' && results.length > 0) {
      e.preventDefault()
      onPick(results[Math.min(active, results.length - 1)].row)
    } else if (e.key === 'Escape' && query) {
      e.preventDefault()
      e.stopPropagation()
      onQuery('')
    }
  }

  return (
    <>
      <div className="kb-settings-search">
        <SearchIcon />
        <input
          type="text"
          role="combobox"
          aria-label="Search settings"
          aria-expanded={results.length > 0}
          aria-controls={results.length > 0 ? LIST_ID : undefined}
          aria-autocomplete="list"
          aria-activedescendant={results.length > 0 ? `${LIST_ID}-${active}` : undefined}
          placeholder="Search settings"
          value={query}
          spellCheck={false}
          autoComplete="off"
          onChange={(e) => {
            setActive(0)
            onQuery(e.target.value)
          }}
          onKeyDown={onKeyDown}
        />
        {query && (
          <button type="button" aria-label="Clear search" onClick={() => onQuery('')}>
            <XIcon />
          </button>
        )}
      </div>
      {query &&
        (results.length === 0 ? (
          <p className="kb-settings-noresults">No settings match</p>
        ) : (
          <ul id={LIST_ID} role="listbox" aria-label="Matching settings" className="kb-settings-results">
            {results.map((found, i) => (
              <li
                key={`${found.section}:${found.title}:${i}`}
                id={`${LIST_ID}-${i}`}
                role="option"
                aria-selected={i === active}
                className={cx({ 'is-active': i === active })}
                onMouseEnter={() => setActive(i)}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => onPick(found.row)}
              >
                <span>{found.title}</span>
                <span>{found.section}</span>
              </li>
            ))}
          </ul>
        ))}
    </>
  )
}
