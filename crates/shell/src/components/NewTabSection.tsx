import { useState } from 'react'

import type { NewTabTarget, UserSettings, UserSettingsPatch } from '@/bindings/types'
import { Segmented } from '@/components/controls'
import { cx } from '@/lib/cx'
import { GripIcon, XIcon } from '@/lib/icons'
import { isSearchQuery, normaliseUrl } from '@/lib/url'
import { usePointerReorder } from '@/hooks/usePointerReorder'
import { reorderByKeys } from '@/lib/shortcuts'

/** Mirrors `wis_core::new_tab::MAX_SHORTCUTS`. */
const MAX_SHORTCUTS = 8

const TARGET_OPTIONS = [
  { value: 'launchpad' as const, label: 'Launchpad' },
  { value: 'homepage' as const, label: 'Homepage' },
] satisfies ReadonlyArray<{ value: NewTabTarget; label: string }>

interface NewTabSectionProps {
  settings: UserSettings
  onUpdate: (patch: UserSettingsPatch) => void
}

export function NewTabSection({ settings, onUpdate }: NewTabSectionProps) {
  // Drafts to commit on blur/Enter instead of on every keystroke (each
  // commit is a persisted settings write).
  const [homepageDraft, setHomepageDraft] = useState(settings.homepage ?? '')
  const [label, setLabel] = useState('')
  const [url, setUrl] = useState('')

  const shortcuts = settings.shortcuts
  const atCap = shortcuts.length >= MAX_SHORTCUTS

  const commitHomepage = () => {
    const next = homepageDraft.trim()
    if (next !== (settings.homepage ?? '')) onUpdate({ homepage: next })
  }

  const addShortcut = () => {
    const raw = url.trim()
    // Only accept real navigable URLs: free text would normalise to a search
    // URL, which a tile should never become.
    if (!raw || atCap || isSearchQuery(raw)) return
    const normalised = normaliseUrl(raw)
    if (!normalised) return
    onUpdate({ shortcuts: [...shortcuts, { label: label.trim(), url: normalised }] })
    setLabel('')
    setUrl('')
  }

  const removeShortcut = (removeUrl: string) => {
    onUpdate({ shortcuts: shortcuts.filter((tile) => tile.url !== removeUrl) })
  }

  const reorder = usePointerReorder({
    keys: shortcuts.map((tile) => tile.url),
    axis: 'y',
    containerSelector: '.kb-shortcut-list',
    ghostClass: 'kb-ghost-row',
    onReorder: (keys) => onUpdate({ shortcuts: reorderByKeys(shortcuts, keys) }),
  })

  return (
    <section id="newtab" className="sec">
      <div className="sec-h">New tab</div>
      <div className="row">
        <div className="l">
          <span>New tabs open</span>
          <span>The launchpad (search and shortcuts) or your homepage.</span>
        </div>
        <Segmented
          value={settings.new_tab_target}
          onChange={(new_tab_target) => onUpdate({ new_tab_target })}
          options={TARGET_OPTIONS}
          ariaLabel="New tab destination"
        />
      </div>
      <div className="row">
        <div className="l">
          <span>Homepage</span>
          <span>Loaded for new tabs while "Homepage" is selected above.</span>
        </div>
        <input
          type="text"
          className="kb-text-input"
          placeholder="example.com"
          value={homepageDraft}
          onChange={(e) => setHomepageDraft(e.currentTarget.value)}
          onBlur={commitHomepage}
          onKeyDown={(e) => {
            if (e.key === 'Enter') e.currentTarget.blur()
          }}
          aria-label="Homepage URL"
        />
      </div>
      <div className="row">
        <div className="l">
          <span>Launchpad shortcuts</span>
          <span>Tiles on the launchpad, up to {MAX_SHORTCUTS}.</span>
        </div>
      </div>
      {shortcuts.length > 0 && (
        <ul className="kb-shortcut-list">
          {shortcuts.map((tile, i) => (
            <li
              key={tile.url}
              data-key={tile.url}
              className={cx('kb-shortcut-item', {
                'is-dragsource': reorder.draggedKey === tile.url,
              })}
              style={{ transform: reorder.itemTransform(i) }}
            >
              <span
                className="kb-shortcut-grip"
                aria-label={`Drag to reorder ${tile.label}`}
                {...reorder.dragHandlers(tile.url)}
              >
                <GripIcon />
              </span>
              <span className="kb-shortcut-label">{tile.label}</span>
              <span className="kb-shortcut-url">{tile.url}</span>
              <div className="kb-shortcut-actions">
                <button
                  type="button"
                  className="kb-iconbtn"
                  onClick={() => removeShortcut(tile.url)}
                  aria-label={`Remove ${tile.label}`}
                >
                  <XIcon />
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
      {atCap ? (
        <div className="kb-shortcut-hint">Remove a shortcut to add another.</div>
      ) : (
        <div className="kb-shortcut-add">
          <input
            type="text"
            className="kb-text-input"
            placeholder="Label (optional)"
            value={label}
            onChange={(e) => setLabel(e.currentTarget.value)}
            aria-label="Shortcut label"
          />
          <input
            type="text"
            className="kb-text-input"
            placeholder="example.com"
            value={url}
            onChange={(e) => setUrl(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') addShortcut()
            }}
            aria-label="Shortcut URL"
          />
          <button type="button" className="kb-btn" onClick={addShortcut} disabled={!url.trim()}>
            Add
          </button>
        </div>
      )}
    </section>
  )
}
