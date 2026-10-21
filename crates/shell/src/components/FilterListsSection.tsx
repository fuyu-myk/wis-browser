/**
 * Settings -> Privacy: filter lists. Shows the built-in sources (EasyPrivacy,
 * EasyList, uBO scriptlets), lets the user subscribe to extra
 * Adblock-Plus-syntax lists by URL (https only; engine-only, never the
 * WKContentRuleList compiler), and offers a manual update of everything.
 */

import { useState } from 'react'

import { commands } from '@/bindings/commands'
import type { UserSettingsPatch } from '@/bindings/types'
import { Switch } from '@/components/controls'
import { XIcon } from '@/lib/icons'
import { noticeListsPatch } from '@/lib/noticeLists'
import { DEFAULT_KEYMAP } from '@/lib/palette'

// Element-picker default, host-localized (Cmd on macOS, Ctrl elsewhere).
const PICKER_SHORTCUT = DEFAULT_KEYMAP.element_picker ?? 'Cmd+Shift+X'

const BUILTINS = [
  { name: 'EasyPrivacy', detail: 'Tracker rules (also compiled for WebKit blocking)' },
  { name: 'EasyList', detail: 'Ad + element-hiding rules' },
  { name: 'uBO scriptlets', detail: "uBlock Origin's library for +js(...) rules, from Ghostery's copy" },
]

interface FilterListsSectionProps {
  blockCookieNotices: boolean
  blockAnnoyances: boolean
  onNoticeListsChange: (patch: UserSettingsPatch) => void
  subscriptions: string[]
  onChange: (next: string[]) => void
  /** Zapped element-hiding rules keyed by host. */
  cosmeticRules: Record<string, string[]>
  onCosmeticChange: (next: Record<string, string[]>) => void
}

export function FilterListsSection({
  blockCookieNotices,
  blockAnnoyances,
  onNoticeListsChange,
  subscriptions,
  onChange,
  cosmeticRules,
  onCosmeticChange,
}: FilterListsSectionProps) {
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

  const cosmeticHosts = Object.keys(cosmeticRules)
    .filter((h) => (cosmeticRules[h] ?? []).length > 0)
    .sort()
  const removeRule = (host: string, selector: string) => {
    const left = (cosmeticRules[host] ?? []).filter((s) => s !== selector)
    const next = { ...cosmeticRules }
    if (left.length === 0) delete next[host]
    else next[host] = left
    onCosmeticChange(next)
  }

  return (
    <>
      <div className="row">
        <div className="l">
          <span>Block cookie notices</span>
          <span>Hides consent banners without answering them.</span>
        </div>
        <Switch
          checked={blockCookieNotices}
          onChange={(on) => onNoticeListsChange(noticeListsPatch('cookie_notices', on))}
          label="Block cookie notices"
        />
      </div>
      <div className="row">
        <div className="l">
          <span>Block annoyances</span>
          <span>Also hides social widgets, newsletter and notification prompts, and app banners.</span>
        </div>
        <Switch
          checked={blockAnnoyances}
          onChange={(on) => onNoticeListsChange(noticeListsPatch('annoyances', on))}
          label="Block annoyances"
        />
      </div>
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
      {cosmeticHosts.length > 0 && (
        <>
          <div className="row">
            <div className="l">
              <span>Hidden elements</span>
              <span>
                Elements you hid with the picker ({PICKER_SHORTCUT}), kept per
                site.
              </span>
            </div>
          </div>
          <ul className="kb-taskman-list">
            {cosmeticHosts.flatMap((host) =>
              (cosmeticRules[host] ?? []).map((selector) => (
                <li className="kb-taskman-row" key={`${host}|${selector}`}>
                  <span className="kb-taskman-kind">{host}</span>
                  <span className="kb-taskman-name" title={selector}>
                    {selector}
                  </span>
                  <button
                    type="button"
                    className="kb-taskman-x"
                    aria-label={`Unhide ${selector} on ${host}`}
                    title="Unhide (reload the page to restore)"
                    onClick={() => removeRule(host, selector)}
                  >
                    <XIcon />
                  </button>
                </li>
              )),
            )}
          </ul>
        </>
      )}
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
