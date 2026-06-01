import {
  useEffect,
  useRef,
  useState,
  type ChangeEvent,
  type FormEvent,
  type KeyboardEvent,
  type MouseEvent,
  type RefObject,
} from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { Tab, WorkspaceId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { SearchIcon } from '@/lib/icons'
import { normaliseUrl } from '@/lib/url'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'
import { FOCUS_URLBAR_EVENT } from '@/hooks/useKeyActions'

/** Hide the http(s):// prefix in the address bar. */
function displayUrl(url: string): string {
  return url.replace(/^https?:\/\//, '')
}

function useUrlBarForm(activeTab: Tab | null, fallbackWorkspaceId: WorkspaceId | null) {
  const qc = useQueryClient()
  const [value, setValue] = useState(displayUrl(activeTab?.url ?? ''))

  useEffect(() => {
    setValue(displayUrl(activeTab?.url ?? ''))
  }, [activeTab?.url, activeTab?.id])

  const navigate = useMutation({
    mutationFn: ({ id, url }: { id: Tab['id']; url: string }) =>
      commands.navigateTab(id, url),
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })

  const create = useMutation({
    mutationFn: ({ workspaceId, url }: { workspaceId: WorkspaceId; url: string }) =>
      commands.createTab(workspaceId, url),
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })

  function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault()
    const target = normaliseUrl(value)
    if (!target) return
    if (activeTab) {
      navigate.mutate({ id: activeTab.id, url: target })
    } else if (fallbackWorkspaceId) {
      create.mutate({ workspaceId: fallbackWorkspaceId, url: target })
    }
  }

  function onChange(e: ChangeEvent<HTMLInputElement>) {
    setValue(e.target.value)
  }

  return { value, onChange, onSubmit }
}

/**
 * Focus + select-all the URL bar input on receipt of the `wis-focus-urlbar`
 * window event (Cmd+L / Cmd+E). Mounted by each visible URL-bar variant; if
 * both are mounted simultaneously the first one wins.
 *
 * `beforeFocus` lets the sidebar variant expand its collapsed shell first
 * so the input has somewhere visible to land. The actual focus is deferred
 * to the next animation frame to give layout time to settle.
 */
function useFocusOnEvent(
  inputRef: RefObject<HTMLInputElement>,
  beforeFocus?: () => void,
) {
  useEffect(() => {
    const onFocus = (event: Event) => {
      const select = (event as CustomEvent<{ select?: boolean }>).detail?.select ?? true
      beforeFocus?.()

      let attempts = 0
      const tryFocus = () => {
        const input = inputRef.current
        if (!input) return
        if (input.offsetParent === null && attempts < 20) {
          attempts += 1
          requestAnimationFrame(tryFocus)
          return
        }
        input.focus()
        if (select) input.select()
      }
      requestAnimationFrame(tryFocus)
    }
    window.addEventListener(FOCUS_URLBAR_EVENT, onFocus as EventListener)
    return () => window.removeEventListener(FOCUS_URLBAR_EVENT, onFocus as EventListener)
  }, [inputRef, beforeFocus])
}

function useSelectAllOnChromeClick(inputRef: RefObject<HTMLInputElement>) {
  const focusedRef = useRef(false)

  useEffect(() => {
    const input = inputRef.current
    if (!input) return
    const onFocus = () => {
      focusedRef.current = true
    }
    const onBlur = () => {
      focusedRef.current = false
    }
    input.addEventListener('focus', onFocus)
    input.addEventListener('blur', onBlur)
    return () => {
      input.removeEventListener('focus', onFocus)
      input.removeEventListener('blur', onBlur)
    }
  }, [inputRef])

  return (e: MouseEvent<HTMLElement>) => {
    const input = inputRef.current
    if (!input) return
    const clickedInput = e.target === input
    if (clickedInput && focusedRef.current) return
    e.preventDefault()
    requestAnimationFrame(() => {
      input.focus()
      input.select()
    })
  }
}

// ─── Sidebar variant ─────────────────────────────────────────────────────

export interface SidebarUrlBarProps {
  activeTab: Tab | null
  fallbackWorkspaceId: WorkspaceId | null
  /** Forwarded to the inner <input> so click-to-expand can focus it. */
  inputRef: RefObject<HTMLInputElement>
  /** Fired on click or focus. */
  onActivate: () => void
  /** Fired after the input blurs on Escape, so the sidebar can collapse. */
  onEscape?: () => void
}

export function SidebarUrlBar({
  activeTab,
  fallbackWorkspaceId,
  inputRef,
  onActivate,
  onEscape,
}: SidebarUrlBarProps) {
  const form = useUrlBarForm(activeTab, fallbackWorkspaceId)
  const selectAll = useSelectAllOnChromeClick(inputRef)
  useFocusOnEvent(inputRef, onActivate)
  const handleMouseDown = (e: MouseEvent<HTMLElement>) => {
    onActivate()
    selectAll(e)
  }
  const handleKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Escape') {
      e.currentTarget.blur()
      onEscape?.()
    }
  }
  return (
    <form
      className="kb-urlbar"
      aria-label="Address bar"
      onMouseDown={handleMouseDown}
      onSubmit={form.onSubmit}
    >
      <SearchIcon className="ico" />
      <input
        ref={inputRef}
        className="kb-urlbar-input"
        type="text"
        value={form.value}
        onChange={form.onChange}
        onFocus={onActivate}
        onKeyDown={handleKeyDown}
        placeholder="Enter search or web address"
        spellCheck={false}
      />
    </form>
  )
}

// ─── Floating / horizontal-pill variant ──────────────────────────────────

export interface FloatingUrlBarProps {
  activeTab: Tab | null
  fallbackWorkspaceId: WorkspaceId | null
  /**
   * When `true`, drop the absolute positioning.
   */
  inline?: boolean
}

export function FloatingUrlBar({
  activeTab,
  fallbackWorkspaceId,
  inline = false,
}: FloatingUrlBarProps) {
  const form = useUrlBarForm(activeTab, fallbackWorkspaceId)
  const inputRef = useRef<HTMLInputElement | null>(null)
  const handleMouseDown = useSelectAllOnChromeClick(inputRef)
  useFocusOnEvent(inputRef)
  const handleKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Escape') e.currentTarget.blur()
  }
  return (
    <form
      className={cx('kb-urlpill-inline', { inline })}
      aria-label="Address bar"
      onMouseDown={handleMouseDown}
      onSubmit={form.onSubmit}
    >
      <SearchIcon className="ico" />
      <input
        ref={inputRef}
        className="kb-urlbar-input"
        type="text"
        value={form.value}
        onChange={form.onChange}
        onKeyDown={handleKeyDown}
        placeholder="Enter search or web address"
        spellCheck={false}
      />
    </form>
  )
}
