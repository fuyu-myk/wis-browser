import { useCallback, useEffect, useRef, useState } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { Tab } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { SearchIcon, XIcon } from '@/lib/icons'
import {
  FIND_CLOSE_EVENT,
  FIND_OPEN_EVENT,
  FIND_STEP_EVENT,
} from '@/hooks/useKeyActions'

const FIND_RESULT_EVENT = 'wis-find-result'

/** Height the FindBar carves out of the WKWebView bounds when open. */
export const FINDBAR_GAP_PX = 64

interface FindBarProps {
  /** Tab the find UI targets. Closes automatically when this changes or
   *  becomes null. */
  activeTab: Tab | null
}

interface FindResultPayload {
  tab_id: string
  query: string
  backwards: boolean
  found: boolean
}

type Status = 'idle' | 'searching' | 'match' | 'no_match'

export function FindBar({ activeTab }: FindBarProps) {
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [status, setStatus] = useState<Status>('idle')
  const inputRef = useRef<HTMLInputElement | null>(null)
  const activeTabIdRef = useRef<string | null>(null)
  activeTabIdRef.current = activeTab?.id ?? null

  const close = useCallback(() => {
    setOpen(false)
    setStatus('idle')
    setQuery('')
    window.dispatchEvent(new CustomEvent(FIND_CLOSE_EVENT))
    if (activeTabIdRef.current) {
      const id = activeTabIdRef.current as Tab['id']
      void commands.tabFindClear(id).catch(() => undefined)
    }
  }, [])

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

  // Open / step keybindings, dispatched by the menu bus.
  useEffect(() => {
    const onOpen = () => {
      if (!activeTabIdRef.current) return
      setOpen(true)
      
      void commands
        .focusChromeWebview()
        .catch(() => undefined)
        .finally(() => {
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
        })
    }
    const onStep = (event: Event) => {
      if (!open) return
      const backwards = !(event as CustomEvent<{ forward?: boolean }>).detail?.forward
      void find(query, backwards)
    }
    window.addEventListener(FIND_OPEN_EVENT, onOpen)
    window.addEventListener(FIND_STEP_EVENT, onStep as EventListener)
    return () => {
      window.removeEventListener(FIND_OPEN_EVENT, onOpen)
      window.removeEventListener(FIND_STEP_EVENT, onStep as EventListener)
    }
  }, [open, query, find])

  // Match-result event from Rust (fired by the WKWebView completion handler).
  useEffect(() => {
    if (!open) return
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
  }, [open])

  // Close + drop highlights when the user switches tabs.
  useEffect(() => {
    if (!open) return
    return () => {
      if (activeTabIdRef.current) {
        const id = activeTabIdRef.current as Tab['id']
        void commands.tabFindClear(id).catch(() => undefined)
      }
    }
  }, [activeTab?.id, open])

  if (!open) return null

  return (
    <div className="kb-findbar-wrap" role="search">
      <div
        className={cx('kb-findbar-pill', {
          'is-no-match': status === 'no_match',
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
          onChange={(e) => {
            const next = e.target.value
            setQuery(next)
            void find(next, false)
          }}
          onKeyDown={(e) => {
            if (e.key === 'Escape') {
              e.preventDefault()
              close()
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
        onClick={close}
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
