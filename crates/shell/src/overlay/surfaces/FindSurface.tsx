/**
 * Find-in-page UI hosted by the `overlay-find` webview. The chrome calls
 * `setOverlayBounds('find', ...)` to position this webview as a bottom
 * strip floating above the page. This surface owns:
 *   - the search input + match status + prev/next/close buttons
 *   - the call to `tab_find_string` / `tab_find_clear`
 *   - the dispatch from the menu accelerators (`wis-keyaction` for
 *     find_in_page / find_next / find_previous fires in every webview)
 *
 * Active-tab state is read from this webview's own React Query cache via
 * `useBrowserState`. The state mutations the chrome makes propagate here
 * through the existing `browser-state-changed` Tauri event.
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { KeyAction, Tab } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { SearchIcon, XIcon } from '@/lib/icons'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'

const KEYACTION_EVENT = 'wis-keyaction'
const FIND_RESULT_EVENT = 'wis-find-result'
const FIND_FOCUS_EVENT = 'wis-overlay-find-focus'
const FIND_CLOSE_REQUEST_EVENT = 'wis-overlay-find-close'

interface KeyActionPayload {
  action: KeyAction
}

interface FindResultPayload {
  tab_id: string
  query: string
  backwards: boolean
  found: boolean
}

type Status = 'idle' | 'searching' | 'match' | 'no_match'

export function FindSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)
  const activeWorkspace = state?.workspaces.find((w) => w.id === state.active_workspace_id)
  const activeTab =
    activeWorkspace?.active_tab_id != null
      ? state?.tabs.find((t) => t.id === activeWorkspace.active_tab_id) ?? null
      : null
  const activeTabIdRef = useRef<string | null>(null)
  activeTabIdRef.current = activeTab?.id ?? null

  const [query, setQuery] = useState('')
  const [status, setStatus] = useState<Status>('idle')
  const inputRef = useRef<HTMLInputElement | null>(null)

  const find = useCallback(
    async (q: string, backwards: boolean) => {
      const id = activeTabIdRef.current
      if (!id || q.length === 0) {
        setStatus('idle')
        return
      }
      setStatus('searching')
      await commands.tabFindString(id as Tab['id'], q, backwards, false)
    },
    [],
  )

  const requestClose = useCallback(() => {
    setQuery('')
    setStatus('idle')
    if (activeTabIdRef.current) {
      void commands.tabFindClear(activeTabIdRef.current as Tab['id']).catch(() => undefined)
    }
    // Tell the chrome so it stops tracking the overlay as visible (and
    // skips bounds resync on window resize). Tauri events propagate to
    // every webview, including this one's listeners.
    void emit(FIND_CLOSE_REQUEST_EVENT)
    void commands.hideOverlay('find').catch(() => undefined)
  }, [])

  // Focus the input the moment the chrome tells the surface to.
  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<void>(FIND_FOCUS_EVENT, () => {
      void commands.focusWebview('overlay-find').catch(() => undefined)
      const focusInput = () => {
        const el = inputRef.current
        if (!el) return
        el.focus()
        el.select()
      }
      requestAnimationFrame(() => {
        focusInput()
        requestAnimationFrame(focusInput)
      })
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  // Menu accelerators (Cmd+G / Cmd+Shift+G) fire across all webviews.
  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<KeyActionPayload>(KEYACTION_EVENT, (e) => {
      if (e.payload.action === 'find_next') void find(query, false)
      else if (e.payload.action === 'find_previous') void find(query, true)
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [find, query])

  // Match result from the WKWebView completion handler.
  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<FindResultPayload>(FIND_RESULT_EVENT, (e) => {
      if (e.payload.tab_id !== activeTabIdRef.current) return
      setStatus(e.payload.found ? 'match' : 'no_match')
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  const glassOn = state?.settings.glass_enabled ?? true

  return (
    <div className={cx('kb-findbar-wrap', { 'glass-on': glassOn })} role="search">
      <div
        className={cx('kb-findbar-pill', {
          'is-no-match': status === 'no_match',
          'glass-on': glassOn,
        })}
      >
        <SearchIcon className="kb-findbar-icon" />
        <input
          ref={inputRef}
          className="kb-findbar-input"
          type="text"
          value={query}
          placeholder="Find in page"
          spellCheck={false}
          autoFocus
          onChange={(e) => {
            const next = e.target.value
            setQuery(next)
            void find(next, false)
          }}
          onKeyDown={(e) => {
            if (e.key === 'Escape') {
              e.preventDefault()
              requestClose()
            } else if (e.key === 'Enter') {
              e.preventDefault()
              void find(query, e.shiftKey)
            }
          }}
        />
        <span
          className={cx('kb-findbar-status', {
            'is-no-match': status === 'no_match',
          })}
          aria-live="polite"
        >
          {status === 'no_match' && 'No matches found'}
        </span>
      </div>
      <button
        type="button"
        className="kb-findbar-circle"
        aria-label="Previous match"
        onClick={() => void find(query, true)}
      >
        <ChevronIcon up />
      </button>
      <button
        type="button"
        className="kb-findbar-circle"
        aria-label="Next match"
        onClick={() => void find(query, false)}
      >
        <ChevronIcon />
      </button>
      <button
        type="button"
        className="kb-findbar-circle"
        aria-label="Close find"
        onClick={requestClose}
      >
        <XIcon />
      </button>
    </div>
  )
}

interface ChevronProps {
  up?: boolean
}

function ChevronIcon({ up = false }: ChevronProps) {
  return (
    <svg
      width={12}
      height={12}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      {up ? <polyline points="3 10 8 5 13 10" /> : <polyline points="3 6 8 11 13 6" />}
    </svg>
  )
}
