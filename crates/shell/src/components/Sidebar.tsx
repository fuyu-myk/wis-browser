import { useEffect, useRef, useState, type MouseEvent, type RefObject } from 'react'

import type { BrowserState, Tab, UserSettings } from '@/bindings/types'
import { cx } from '@/lib/cx'

import { Brand } from '@/components/Brand'
import { SidebarUrlBar } from '@/components/UrlBar'
import { TabList } from '@/components/TabList'
import { WorkspacePill } from '@/components/WorkspacePill'

interface SidebarProps {
  state: BrowserState
  settings: UserSettings
  /** Lifted to <Window> so the URL bar can also request expansion. */
  expanded: boolean
  setExpanded: (v: boolean) => void
  /** Forwarded to <SidebarUrlBar> so click-to-expand can also focus the input. */
  urlInputRef: RefObject<HTMLInputElement>
}

// Expands on edge hover, empty-area click, or URL bar activation when
// `sidebar_collapsible` is on.
export function Sidebar({
  state,
  settings,
  expanded,
  setExpanded,
  urlInputRef,
}: SidebarProps) {
  const activeWorkspace = state.workspaces.find((w) => w.id === state.active_workspace_id)
  const tabs: Tab[] = activeWorkspace
    ? activeWorkspace.tab_order
        .map((id) => state.tabs.find((t) => t.id === id))
        .filter((t): t is Tab => t !== undefined)
    : []
  const activeTab = tabs.find((t) => t.id === activeWorkspace?.active_tab_id) ?? null

  const showUrlBar = settings.url_bar_position === 'sidebar'
  const collapsible = settings.sidebar_collapsible

  const asideRef = useRef<HTMLElement>(null)

  const [workspacePopoverOpen, setWorkspacePopoverOpen] = useState(false)
  useEffect(() => {
    if (workspacePopoverOpen) setExpanded(true)
  }, [workspacePopoverOpen, setExpanded])

  useEffect(() => {
    if (!collapsible || !expanded) return
    // Esc and explicit click-out close the workspace popover and
    // collapse the sidebar in a single gesture.
    const collapse = () => {
      window.dispatchEvent(new CustomEvent('wis-close-workspace-popover'))
      setWorkspacePopoverOpen(false)
      setExpanded(false)
    }
    const onPointerDown = (e: PointerEvent) => {
      const aside = asideRef.current
      if (!aside) return
      const target = e.target as Element | null
      if (aside.contains(target)) return
      if (target?.closest?.('.kb-ws-pop')) return
      collapse()
    }
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        collapse()
      }
    }
    document.addEventListener('pointerdown', onPointerDown)
    document.addEventListener('keydown', onKeyDown)
    window.addEventListener('blur', collapse)
    return () => {
      document.removeEventListener('pointerdown', onPointerDown)
      document.removeEventListener('keydown', onKeyDown)
      window.removeEventListener('blur', collapse)
    }
  }, [collapsible, expanded, setExpanded])

  const handleAsideClick = (e: MouseEvent<HTMLElement>) => {
    if (!collapsible || expanded) return
    if (e.target === e.currentTarget) {
      setExpanded(true)
    }
  }

  const handleUrlBarActivate = () => {
    if (collapsible && !expanded) {
      setExpanded(true)
      requestAnimationFrame(() => urlInputRef.current?.focus())
    } else {
      urlInputRef.current?.focus()
    }
  }

  return (
    <aside
      ref={asideRef}
      className={cx('kb-side', { 'is-expanded': expanded })}
      onMouseLeave={() => collapsible && !workspacePopoverOpen && setExpanded(false)}
      onClick={handleAsideClick}
    >
      {collapsible && (
        <div
          className="kb-side-edge"
          aria-hidden
          onMouseEnter={() => setExpanded(true)}
        />
      )}
      <Brand />
      {showUrlBar && (
        <SidebarUrlBar
          activeTab={activeTab}
          fallbackWorkspaceId={activeWorkspace?.id ?? null}
          inputRef={urlInputRef}
          onActivate={handleUrlBarActivate}
          onEscape={() => collapsible && setExpanded(false)}
        />
      )}
      <WorkspacePill state={state} onOpenChange={setWorkspacePopoverOpen} />
      <TabList
        workspaceId={activeWorkspace?.id ?? ('' as never)}
        tabs={tabs}
        activeTabId={activeTab?.id ?? null}
      />
    </aside>
  )
}
