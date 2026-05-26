import {
  useEffect,
  useRef,
  useState,
  type ChangeEvent,
  type FormEvent,
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
}

export function SidebarUrlBar({
  activeTab,
  fallbackWorkspaceId,
  inputRef,
  onActivate,
}: SidebarUrlBarProps) {
  const form = useUrlBarForm(activeTab, fallbackWorkspaceId)
  const selectAll = useSelectAllOnChromeClick(inputRef)
  const handleMouseDown = (e: MouseEvent<HTMLElement>) => {
    onActivate()
    selectAll(e)
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
        placeholder="Enter search or web address"
        spellCheck={false}
      />
    </form>
  )
}
