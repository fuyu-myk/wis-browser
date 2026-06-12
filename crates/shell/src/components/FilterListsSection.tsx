/**
 * Settings -> Privacy: filter lists. Shows the built-in sources (EasyPrivacy,
 * EasyList, uBO scriptlet resources), lets the user subscribe to extra
 * Adblock-Plus-syntax lists by URL (https only; engine-only, never the
 * WKContentRuleList compiler), and offers a manual update of everything.
 */

import { useState } from 'react'

import { commands } from '@/bindings/commands'
import { XIcon } from '@/lib/icons'

const BUILTINS = [
  { name: 'EasyPrivacy', detail: 'Tracker rules (also compiled for WebKit blocking)' },
  { name: 'EasyList', detail: 'Ad + element-hiding rules' },
  { name: 'uBO scriptlet resources', detail: 'Script library expanding +js(...) rules' },
]

interface FilterListsSectionProps {
  subscriptions: string[]
  onChange: (next: string[]) => void
}

export function FilterListsSection({ subscriptions, onChange }: FilterListsSectionProps) {
  const [draft, setDraft] = useState('')
  const [status, setStatus] = useState<string | null>(null)

  // Fetches the new list (and anything else) then rebuilds the engine; the
  // page's "Refresh now" row drives the same command for manual updates.
  const refresh = () => {
    void commands.refreshFilterLists().catch(() => setStatus('Fetch failed'))
  }

  const add = () => {
    const url = draft.trim()
    if (!url.startsWith('https://')) {
      setStatus('Subscription URLs must start with https://')
      return
    }
    if (subscriptions.includes(url)) {
      setDraft('')
      return
    }
    onChange([...subscriptions, url])
    setDraft('')
    refresh()
  }

  return (
    <>
      <div className="row">
        <div className="l">
          <span>Filter lists</span>
          <span>
            Ad and tracker rules refresh daily in the background. Subscribed
            lists feed the blocking engine alongside the built-ins.
          </span>
        </div>
      </div>
      {status !== null && <p className="kb-filterlists-status">{status}</p>}
      <ul className="kb-taskman-list">
        {BUILTINS.map((b) => (
          <li className="kb-taskman-row" key={b.name}>
            <span className="kb-taskman-kind">Built in</span>
            <span className="kb-taskman-name">{b.name}</span>
            <span className="kb-filterlists-detail">{b.detail}</span>
            <span className="kb-taskman-noaction" aria-hidden />
          </li>
        ))}
        {subscriptions.map((url) => (
          <li className="kb-taskman-row" key={url}>
            <span className="kb-taskman-kind">Custom</span>
            <span className="kb-taskman-name" title={url}>
              {url}
            </span>
            <button
              type="button"
              className="kb-taskman-x"
              aria-label={`Remove ${url}`}
              title="Remove"
              onClick={() => onChange(subscriptions.filter((u) => u !== url))}
            >
              <XIcon />
            </button>
          </li>
        ))}
      </ul>
      <div className="row kb-filterlists-add">
        <input
          type="text"
          className="kb-text-input"
          placeholder="https://example.com/list.txt"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') add()
          }}
          aria-label="Subscription URL"
        />
        <button type="button" className="kb-btn" onClick={add} disabled={draft.trim() === ''}>
          Add
        </button>
      </div>
    </>
  )
}
