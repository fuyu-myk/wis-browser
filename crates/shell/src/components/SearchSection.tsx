import { useState } from 'react'

import type {
  SearchEngine,
  UserSettings,
  UserSettingsPatch,
} from '@/bindings/types'
import { SEARCH_ENGINES } from '@/lib/url'

/** Mirrors `wis_core::search::MAX_CUSTOM_ENGINES`. */
const MAX_CUSTOM_ENGINES = 16

const BUILTINS: readonly SearchEngine[] = ['duckduckgo', 'google', 'bing']

interface SearchSectionProps {
  settings: UserSettings
  onUpdate: (patch: UserSettingsPatch) => void
}

interface KeywordFieldProps {
  /** Engine display name, for labels. */
  label: string
  value: string
  onCommit: (raw: string) => void
}

/** Keyword shown as a static badge with an Edit affordance; clicking spawns
 *  the compact input. Enter or click-away saves (a persisted settings write),
 *  Escape cancels. A rejected save reverts to the stored value. */
function KeywordField({ label, value, onCommit }: KeywordFieldProps) {
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState('')

  if (!editing) {
    return (
      <>
        {value && <span className="kb-ua-prof">{value}</span>}
        <button
          type="button"
          className="kb-ua-remove"
          onClick={() => {
            setDraft(value)
            setEditing(true)
          }}
          aria-label={`${value ? 'Edit' : 'Add'} keyword for ${label}`}
        >
          {value ? 'Edit' : 'Add keyword'}
        </button>
      </>
    )
  }

  return (
    <input
      type="text"
      className="kb-text-input kb-kw-input"
      placeholder="keyword"
      value={draft}
      autoFocus
      onChange={(e) => setDraft(e.currentTarget.value)}
      onBlur={() => {
        setEditing(false)
        onCommit(draft)
      }}
      onKeyDown={(e) => {
        if (e.key === 'Enter') e.currentTarget.blur()
        if (e.key === 'Escape') setEditing(false)
      }}
      spellCheck={false}
      aria-label={`Keyword for ${label}`}
    />
  )
}

export function SearchSection({ settings, onUpdate }: SearchSectionProps) {
  const [name, setName] = useState('')
  const [keyword, setKeyword] = useState('')
  const [template, setTemplate] = useState('')
  const [status, setStatus] = useState<string | null>(null)

  const engines = settings.custom_search_engines
  const builtinKeywords = settings.builtin_search_keywords
  const customDefault = engines.find((e) => e.is_default) ?? null
  const atCap = engines.length >= MAX_CUSTOM_ENGINES

  /** Single-word, capped, unique across both engine lists. Returns the
   *  normalised keyword, or null after setting a status message. */
  const validKeyword = (raw: string, ownedBy: (kw: string) => boolean): string | null => {
    const kw = raw.trim().toLowerCase()
    if (/\s/.test(kw) || kw.length > 32) {
      setStatus('Keyword must be a single word.')
      return null
    }
    const taken =
      engines.some((e) => e.keyword === kw && !ownedBy(kw)) ||
      BUILTINS.some((e) => builtinKeywords[e] === kw && !ownedBy(kw))
    if (taken) {
      setStatus('Keyword already used.')
      return null
    }
    return kw
  }

  const commitBuiltinKeyword = (engine: SearchEngine, raw: string) => {
    setStatus(null)
    const current = builtinKeywords[engine] ?? ''
    const trimmed = raw.trim().toLowerCase()
    if (trimmed === current) return
    const next = { ...builtinKeywords }
    if (!trimmed) {
      delete next[engine]
      onUpdate({ builtin_search_keywords: next })
      return
    }
    const kw = validKeyword(raw, (k) => builtinKeywords[engine] === k)
    if (!kw) return
    next[engine] = kw
    onUpdate({ builtin_search_keywords: next })
  }

  const commitCustomKeyword = (original: string, raw: string) => {
    setStatus(null)
    const trimmed = raw.trim().toLowerCase()
    if (trimmed === original) return
    if (!trimmed) {
      setStatus('Custom engines need a keyword.')
      return
    }
    const kw = validKeyword(raw, (k) => k === original)
    if (!kw) return
    onUpdate({
      custom_search_engines: engines.map((e) =>
        e.keyword === original ? { ...e, keyword: kw } : e,
      ),
    })
  }

  const chooseBuiltin = (engine: SearchEngine) => {
    onUpdate({
      search_engine: engine,
      custom_search_engines: engines.map((e) => ({ ...e, is_default: false })),
    })
  }

  const chooseCustom = (kw: string) => {
    onUpdate({
      custom_search_engines: engines.map((e) => ({ ...e, is_default: e.keyword === kw })),
    })
  }

  const removeEngine = (kw: string) => {
    onUpdate({ custom_search_engines: engines.filter((e) => e.keyword !== kw) })
  }

  const addEngine = () => {
    setStatus(null)
    const kw = keyword.trim().toLowerCase()
    const tpl = template.trim()
    if (!kw || /\s/.test(kw)) {
      setStatus('Keyword must be a single word.')
      return
    }
    if (engines.some((e) => e.keyword === kw)) {
      setStatus('Keyword already used.')
      return
    }
    if (kw.length > 32) {
      setStatus('Keyword is too long.')
      return
    }
    if (!/^https?:\/\//i.test(tpl)) {
      setStatus('URL must start with http:// or https://.')
      return
    }
    if (!tpl.includes('{searchTerms}') && !tpl.includes('%s')) {
      setStatus('URL must contain %s (or {searchTerms}) where the query goes.')
      return
    }
    if (tpl.length > 2048) {
      setStatus('URL is too long.')
      return
    }
    // Mirrors the boundary sanitizer's checks.
    const probeUrl = (probe: string) => {
      try {
        return new URL(tpl.replace(/\{searchTerms\}|%s/g, probe))
      } catch {
        return null
      }
    }
    const probeA = probeUrl('wisprobea')
    const probeB = probeUrl('wisprobeb')
    if (!probeA || !probeB) {
      setStatus("That URL doesn't look valid.")
      return
    }
    if (
      probeA.origin !== probeB.origin ||
      probeA.username !== probeB.username ||
      probeA.password !== probeB.password
    ) {
      setStatus("The query placeholder can't be part of the site address.")
      return
    }
    // The boundary sanitizer normalises %s, derives an empty name from the
    // host, and enforces the caps authoritatively.
    onUpdate({
      custom_search_engines: [
        ...engines,
        { name: name.trim(), keyword: kw, template: tpl, is_default: false },
      ],
    })
    setName('')
    setKeyword('')
    setTemplate('')
  }

  return (
    <section id="search" className="sec">
      <div className="sec-h">Search</div>
      <div className="row">
        <div className="l">
          <span>Default engine</span>
          <span>
            Used for address-bar searches and suggestions. Any engine with a
            keyword can also be triggered anytime by typing it before a query.
          </span>
        </div>
      </div>
      <ul className="kb-ua-list">
        {BUILTINS.map((engine) => (
          <li key={engine} className="kb-ua-item">
            <span className="kb-ua-host">{SEARCH_ENGINES[engine].label}</span>
            <KeywordField
              label={SEARCH_ENGINES[engine].label}
              value={builtinKeywords[engine] ?? ''}
              onCommit={(raw) => commitBuiltinKeyword(engine, raw)}
            />
            {!customDefault && settings.search_engine === engine ? (
              <span className="kb-ua-prof">Default</span>
            ) : (
              <button
                type="button"
                className="kb-ua-remove"
                onClick={() => chooseBuiltin(engine)}
                aria-label={`Make ${SEARCH_ENGINES[engine].label} the default engine`}
              >
                Make default
              </button>
            )}
          </li>
        ))}
        {engines.map((engine) => (
          <li key={engine.keyword} className="kb-ua-item">
            <span className="kb-ua-host">{engine.name}</span>
            <KeywordField
              label={engine.name}
              value={engine.keyword}
              onCommit={(raw) => commitCustomKeyword(engine.keyword, raw)}
            />
            {engine.is_default ? (
              <span className="kb-ua-prof">Default</span>
            ) : (
              <button
                type="button"
                className="kb-ua-remove"
                onClick={() => chooseCustom(engine.keyword)}
                aria-label={`Make ${engine.name} the default engine`}
              >
                Make default
              </button>
            )}
            <button
              type="button"
              className="kb-ua-remove"
              onClick={() => removeEngine(engine.keyword)}
              aria-label={`Remove ${engine.name}`}
            >
              Remove
            </button>
          </li>
        ))}
      </ul>
      <div className="row">
        <div className="l">
          <span>Add a custom engine</span>
          <span>
            Paste the site&apos;s search URL with %s in place of the query, e.g.
            https://kagi.com/search?q=%s.
          </span>
        </div>
      </div>
      {atCap ? (
        <p className="kb-backup-status">Up to {MAX_CUSTOM_ENGINES} custom engines.</p>
      ) : (
        <div className="kb-ua-add">
          <input
            type="text"
            className="kb-text-input"
            placeholder="Name (optional)"
            value={name}
            onChange={(e) => setName(e.currentTarget.value)}
            aria-label="Engine name"
          />
          <input
            type="text"
            className="kb-text-input"
            placeholder="keyword"
            value={keyword}
            onChange={(e) => setKeyword(e.currentTarget.value)}
            spellCheck={false}
            aria-label="Engine keyword"
          />
          <input
            type="text"
            className="kb-text-input"
            placeholder="https://example.com/search?q=%s"
            value={template}
            onChange={(e) => setTemplate(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') addEngine()
            }}
            spellCheck={false}
            aria-label="Search URL template"
          />
          <button
            type="button"
            className="kb-btn"
            onClick={addEngine}
            disabled={!keyword.trim() || !template.trim()}
          >
            Add
          </button>
        </div>
      )}
      {status && <p className="kb-backup-status is-error">{status}</p>}
    </section>
  )
}
