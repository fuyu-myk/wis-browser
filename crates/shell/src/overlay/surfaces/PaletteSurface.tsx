/**
 * Command palette UI hosted by the `overlay-palette` webview. The
 * chrome sizes this webview to fill the window so the dimmed backdrop
 * captures click-outside. The palette itself is a centered floating
 * panel; the backdrop is the rest of the overlay surface.
 *
 * Activation routing:
 *   - Tab items call `set_active_tab` (and `set_active_workspace` first
 *     when the tab belongs to a different workspace).
 *   - Action items re-emit the `wis-keyaction` Tauri event with the
 *     chosen action; the chrome's `useKeyActions` handler runs it.
 *     Keeping action dispatch single-sourced means a palette-driven
 *     "New tab" behaves identically to its menu accelerator.
 *   - "Open" items create a new tab from `resolveInput(query, settings)`
 *     (custom-engine keywords included) so the palette doubles as a
 *     launcher when nothing else matches.
 *   - Cmd+Backspace on a tab row invokes its secondary action (close)
 *     without dismissing the palette so users can sweep stale tabs.
 */

import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { KeyboardEvent } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

const SINK_MS = 220

import { commands } from '@/bindings/commands'
import type { KeyAction, Tab, UserSettingsPatch } from '@/bindings/types'
import { cx } from '@/lib/cx'
import {
  effectiveKeymap,
  historyKey,
  makeActionsSource,
  makeClosedTabsSource,
  makeBookmarksSource,
  makeHistorySource,
  makeNavigateSource,
  makeSettingsTogglesSource,
  makeTabsSource,
  type PaletteItem,
} from '@/lib/palette'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'
import { menusGlass } from '@/lib/glass'

const KEYACTION_EVENT = 'wis-keyaction'
const PALETTE_FOCUS_EVENT = 'wis-overlay-palette-focus'
const PALETTE_CLOSED_EVENT = 'wis-overlay-palette-closed'

/** How much history to pull per search; the source shows the best few. */
const HISTORY_LIMIT = 30
/** Minimum query length (characters) before history is matched. */
const HISTORY_MIN_QUERY = 2
const HISTORY_SETTLE_MS = 120
/** Time before bookmarks are considered stale. */
const BOOKMARKS_STALE_MS = 30_000
/** How many closed tabs to pull. */
const CLOSED_TAB_LIMIT = 25
/** Matched by prefix. */
const CLOSED_TABS_KEY = ['palette-closed-tabs']

interface SectionedItems {
  tabs: PaletteItem[]
  closed: PaletteItem[]
  bookmarks: PaletteItem[]
  history: PaletteItem[]
  actions: PaletteItem[]
  settings: PaletteItem[]
  open: PaletteItem[]
  flat: PaletteItem[]
}

/**
 * Hold a value still until it stops changing, so typing does not fire one
 * storage search per keystroke.
 */
function useSettled<T>(value: T, delayMs: number): T {
  const [settled, setSettled] = useState(value)
  useEffect(() => {
    const timer = window.setTimeout(() => setSettled(value), delayMs)
    return () => window.clearTimeout(timer)
  }, [value, delayMs])
  return settled
}

export function PaletteSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)

  const [query, setQuery] = useState('')
  const [highlightId, setHighlightId] = useState<string | null>(null)
  const inputRef = useRef<HTMLInputElement | null>(null)
  const listRef = useRef<HTMLDivElement | null>(null)

  const [closing, setClosing] = useState(false)
  const [openNonce, setOpenNonce] = useState(0)
  const [showing, setShowing] = useState(false)
  const close = useCallback(async () => {
    // Sink back to the bottom edge before the overlay hides.
    setClosing(true)
    await new Promise((resolve) => setTimeout(resolve, SINK_MS))
    setClosing(false)
    setQuery('')
    setHighlightId(null)
    setShowing(false)
    await commands.hideOverlay('palette').catch(() => undefined)
    await emit(PALETTE_CLOSED_EVENT).catch(() => undefined)
  }, [])

  const activateTab = useCallback(
    async (tab: Tab) => {
      if (!state) return
      const owningWorkspace = state.workspaces.find((w) =>
        w.tab_order.includes(tab.id),
      )
      if (owningWorkspace && owningWorkspace.id !== state.active_workspace_id) {
        await commands.setActiveWorkspace(owningWorkspace.id)
      }
      await commands.setActiveTab(tab.id)
      await close()
    },
    [state, close],
  )

  // Close the tab in place. The list re-renders without
  // that tab once `browser-state-changed` fires.
  const queries = useQueryClient()

  const closeTab = useCallback(async (tab: Tab) => {
    await commands.closeTab(tab.id).catch(() => undefined)
  }, [])

  const dispatchAction = useCallback(
    async (action: KeyAction) => {
      await close()
      await emit(KEYACTION_EVENT, { action }).catch(() => undefined)
    },
    [close],
  )

  const openUrl = useCallback(
    async (url: string) => {
      if (!state) return
      const ws = state.workspaces.find((w) => w.id === state.active_workspace_id)
      if (!ws) return
      await commands.createTab(ws.id, url)
      await close()
    },
    [state, close],
  )

  const reopenClosed = useCallback(
    async (id: number) => {
      await commands.reopenClosedTab(id).catch(() => undefined)
      await close()
    },
    [close],
  )

  const updateSettings = useCallback(async (patch: UserSettingsPatch) => {
    await commands.updateSettings(patch).catch(() => undefined)
  }, [])

  const workspaceId = state?.active_workspace_id ?? null
  const settledQuery = useSettled(query.trim(), HISTORY_SETTLE_MS)

  // A private window does not record or reopen closed tabs.
  const workspacePrivate =
    state?.workspaces.find((ws) => ws.id === state.active_workspace_id)?.ephemeral ?? false

  // Refetched each time the palette opens.
  const { data: closedTabs } = useQuery({
    queryKey: [...CLOSED_TABS_KEY, openNonce],
    queryFn: () => commands.listClosedTabs(CLOSED_TAB_LIMIT),
    enabled: !workspacePrivate,
  })

  const { data: bookmarks } = useQuery({
    queryKey: ['palette-bookmarks', workspaceId],
    queryFn: () =>
      workspaceId === null ? Promise.resolve([]) : commands.listBookmarks(workspaceId),
    enabled: workspaceId !== null,
    staleTime: BOOKMARKS_STALE_MS,
  })

  const historyReady = workspaceId !== null && settledQuery.length >= HISTORY_MIN_QUERY

  const { data: history } = useQuery({
    // Scoped to the workspace.
    queryKey: ['palette-history', workspaceId, settledQuery],
    queryFn: () =>
      commands.getHistory({
        search: settledQuery,
        workspace_id: workspaceId,
        limit: HISTORY_LIMIT,
      }),
    enabled: historyReady,
    // Hold the last results while the next search runs.
    placeholderData: (previous) => previous,
  })

  // `placeholderData` fires whenever a key has no data of its own.
  const historyEntries = useMemo(() => (historyReady ? (history ?? []) : []), [historyReady, history])

  const sources = useMemo(() => {
    if (!state) return []
    return [
      makeTabsSource({ state, onActivate: activateTab, onClose: closeTab }),
      makeClosedTabsSource({ entries: closedTabs ?? [], onReopen: reopenClosed }),
      makeBookmarksSource({ bookmarks: bookmarks ?? [], onOpen: openUrl }),
      makeHistorySource({ entries: historyEntries, onOpen: openUrl }),
      makeActionsSource({
        keymap: effectiveKeymap(state.settings.keybindings),
        onDispatch: dispatchAction,
      }),
      makeSettingsTogglesSource({ settings: state.settings, onChange: updateSettings }),
      makeNavigateSource({ onOpen: openUrl, settings: state.settings }),
    ]
  }, [
    state,
    activateTab,
    closeTab,
    dispatchAction,
    openUrl,
    updateSettings,
    bookmarks,
    historyEntries,
    closedTabs,
    reopenClosed,
  ])

  const tabIds = (state?.tabs ?? []).map((tab) => tab.id).join('|')
  const previousTabIds = useRef(tabIds)
  useEffect(() => {
    const before = previousTabIds.current.split('|').filter(Boolean)
    const after = new Set(tabIds.split('|').filter(Boolean))
    previousTabIds.current = tabIds
    // Only while open.
    if (showing && before.some((id) => !after.has(id))) {
      void queries.invalidateQueries({ queryKey: CLOSED_TABS_KEY })
    }
  }, [tabIds, showing, queries])

  const sectioned: SectionedItems = useMemo(() => {
    const all = sources.flatMap((src) => src(query))
    const tabs = all.filter((it) => it.section === 'tabs')
    const closed = all.filter((it) => it.section === 'closed')
    const bookmarks = all.filter((it) => it.section === 'bookmarks')
    // A page shown as closed or bookmarked is dropped from history.
    const shown = new Set(
      [...closed, ...bookmarks].map((item) => historyKey(item.subtitle ?? '')),
    )
    const history = all
      .filter((it) => it.section === 'history')
      .filter((it) => !shown.has(historyKey(it.subtitle ?? '')))
    const actions = all.filter((it) => it.section === 'actions')
    const settings = all.filter((it) => it.section === 'settings')
    const open = all.filter((it) => it.section === 'open')
    return {
      tabs,
      closed,
      bookmarks,
      history,
      actions,
      settings,
      open,
      // Pages the user has already committed to first, most-committed
      // downward: open, then just closed, then saved, then merely visited.
      flat: [...tabs, ...closed, ...bookmarks, ...history, ...actions, ...settings, ...open],
    }
  }, [sources, query])

  const highlight = useMemo(() => {
    if (highlightId === null) return 0
    const index = sectioned.flat.findIndex((item) => item.id === highlightId)
    return index >= 0 ? index : 0
  }, [highlightId, sectioned.flat])

  const moveHighlight = useCallback(
    (delta: number) => {
      const last = sectioned.flat.length - 1
      if (last < 0) return
      const next = Math.min(Math.max(highlight + delta, 0), last)
      setHighlightId(sectioned.flat[next]?.id ?? null)
    },
    [highlight, sectioned.flat],
  )

  useEffect(() => {
    setHighlightId(null)
  }, [query])

  useEffect(() => {
    const container = listRef.current
    if (!container) return
    const el = container.querySelector<HTMLElement>(
      `[data-palette-index="${highlight}"]`,
    )
    if (!el) return
    el.scrollIntoView({ block: 'nearest' })
  }, [highlight, sectioned.flat.length])

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<void>(PALETTE_FOCUS_EVENT, () => {
      setOpenNonce((n) => n + 1)
      setShowing(true)
      void commands.focusSelf().catch(() => undefined)
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

  const activate = useCallback((item: PaletteItem) => {
    void item.activate()
  }, [])

  const runSecondary = useCallback((item: PaletteItem) => {
    if (!item.secondaryAction) return
    void item.secondaryAction.run()
  }, [])

  const onInputKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Escape') {
      e.preventDefault()
      void close()
      return
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      moveHighlight(1)
      return
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault()
      moveHighlight(-1)
      return
    }
    // Cmd+Backspace: row-scoped secondary action (close tab from palette).
    if ((e.metaKey || e.ctrlKey) && e.key === 'Backspace') {
      const item = sectioned.flat[highlight]
      if (item?.secondaryAction) {
        e.preventDefault()
        runSecondary(item)
      }
      return
    }
    if (e.key === 'Enter') {
      e.preventDefault()
      const item = sectioned.flat[highlight]
      if (item) activate(item)
    }
  }

  const glassMenus = menusGlass(state?.settings)

  return (
    <div
      className={cx('kb-palette-backdrop', { 'glass-menus': glassMenus, 'is-closing': closing })}
      role="presentation"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) void close()
      }}
    >
      <div
        key={openNonce}
        className={cx('kb-palette', { 'glass-menus': glassMenus, 'is-closing': closing })}
        role="dialog"
        aria-label="Command palette"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="kb-palette-input-wrap">
          <input
            ref={inputRef}
            className="kb-palette-input"
            type="text"
            value={query}
            placeholder="Search tabs, run actions, or paste a URL..."
            spellCheck={false}
            autoComplete="off"
            autoCorrect="off"
            autoCapitalize="off"
            autoFocus
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onInputKeyDown}
          />
        </div>
        <div className="kb-palette-results" ref={listRef}>
          {sectioned.tabs.length > 0 && (
            <Section label="Tabs">
              {sectioned.tabs.map((item) => {
                const idx = sectioned.flat.indexOf(item)
                return (
                  <ItemRow
                    key={item.id}
                    item={item}
                    index={idx}
                    highlighted={idx === highlight}
                    onActivate={() => activate(item)}
                  />
                )
              })}
            </Section>
          )}
          {sectioned.closed.length > 0 && (
            <Section label="Recently closed">
              {sectioned.closed.map((item) => {
                const idx = sectioned.flat.indexOf(item)
                return (
                  <ItemRow
                    key={item.id}
                    item={item}
                    index={idx}
                    highlighted={idx === highlight}
                    onActivate={() => activate(item)}
                  />
                )
              })}
            </Section>
          )}
          {sectioned.bookmarks.length > 0 && (
            <Section label="Bookmarks">
              {sectioned.bookmarks.map((item) => {
                const idx = sectioned.flat.indexOf(item)
                return (
                  <ItemRow
                    key={item.id}
                    item={item}
                    index={idx}
                    highlighted={idx === highlight}
                    onActivate={() => activate(item)}
                  />
                )
              })}
            </Section>
          )}
          {sectioned.history.length > 0 && (
            <Section label="History">
              {sectioned.history.map((item) => {
                const idx = sectioned.flat.indexOf(item)
                return (
                  <ItemRow
                    key={item.id}
                    item={item}
                    index={idx}
                    highlighted={idx === highlight}
                    onActivate={() => activate(item)}
                  />
                )
              })}
            </Section>
          )}
          {sectioned.actions.length > 0 && (
            <Section label="Actions">
              {sectioned.actions.map((item) => {
                const idx = sectioned.flat.indexOf(item)
                return (
                  <ItemRow
                    key={item.id}
                    item={item}
                    index={idx}
                    highlighted={idx === highlight}
                    onActivate={() => activate(item)}
                  />
                )
              })}
            </Section>
          )}
          {sectioned.settings.length > 0 && (
            <Section label="Settings">
              {sectioned.settings.map((item) => {
                const idx = sectioned.flat.indexOf(item)
                return (
                  <ItemRow
                    key={item.id}
                    item={item}
                    index={idx}
                    highlighted={idx === highlight}
                    onActivate={() => activate(item)}
                  />
                )
              })}
            </Section>
          )}
          {sectioned.open.length > 0 && (
            <Section label="Open">
              {sectioned.open.map((item) => {
                const idx = sectioned.flat.indexOf(item)
                return (
                  <ItemRow
                    key={item.id}
                    item={item}
                    index={idx}
                    highlighted={idx === highlight}
                    onActivate={() => activate(item)}
                  />
                )
              })}
            </Section>
          )}
          {sectioned.flat.length === 0 && (
            <div className="kb-palette-empty">No results</div>
          )}
        </div>
      </div>
    </div>
  )
}

function Section({
  label,
  children,
}: {
  label: string
  children: React.ReactNode
}) {
  return (
    <div className="kb-palette-section">
      <div className="kb-palette-section-label">{label}</div>
      <div className="kb-palette-section-body">{children}</div>
    </div>
  )
}

interface RowProps {
  item: PaletteItem
  index: number
  highlighted: boolean
  onActivate: () => void
}

function ItemRow({ item, index, highlighted, onActivate }: RowProps) {
  const showSecondary = highlighted && item.secondaryAction
  return (
    <button
      type="button"
      data-palette-index={index}
      className={cx('kb-palette-row', { highlighted })}
      onMouseDown={(e) => e.preventDefault()}
      onClick={onActivate}
    >
      <span className="kb-palette-row-main">
        <span className="kb-palette-row-title">
          <HighlightedText text={item.title} matches={item.titleMatches} />
        </span>
        {item.subtitle && (
          <span className="kb-palette-row-subtitle">
            <HighlightedText text={item.subtitle} matches={item.subtitleMatches} />
          </span>
        )}
      </span>
      {item.tabMeta && (
        <span
          className={cx('kb-palette-row-workspace', {
            'is-cross': item.tabMeta.crossWorkspace,
          })}
          title={
            item.tabMeta.crossWorkspace
              ? `In workspace "${item.tabMeta.workspaceName}"`
              : item.tabMeta.workspaceName
          }
        >
          <span
            className={cx('kb-palette-row-workspace-dot', `accent-${item.tabMeta.workspaceAccent}`)}
            aria-hidden
          />
          <span className="kb-palette-row-workspace-name">
            {item.tabMeta.workspaceName}
          </span>
        </span>
      )}
      {showSecondary && item.secondaryAction && (
        <span
          className="kb-palette-row-secondary"
          title={item.secondaryAction.label}
        >
          <span className="kb-palette-row-secondary-label">
            {item.secondaryAction.label}
          </span>
          <span className="kb-palette-row-secondary-shortcut">
            {item.secondaryAction.shortcut}
          </span>
        </span>
      )}
      {item.accessory && (
        <span className="kb-palette-row-accessory">{item.accessory}</span>
      )}
    </button>
  )
}

/**
 * Renders text with the given `matches` (haystack indices) bolded. When
 * no matches are supplied the text renders verbatim. Matched chars get
 * grouped into runs so consecutive matches render as a single span and
 * the DOM stays small.
 */
function HighlightedText({
  text,
  matches,
}: {
  text: string
  matches?: number[]
}) {
  if (!matches || matches.length === 0) return <>{text}</>
  const set = new Set(matches)

  const runs: Array<{ text: string; matched: boolean }> = []
  let current: { text: string; matched: boolean } | null = null
  for (let i = 0; i < text.length; i++) {
    const matched = set.has(i)
    if (!current || current.matched !== matched) {
      current = { text: text[i], matched }
      runs.push(current)
    } else {
      current.text += text[i]
    }
  }

  return (
    <>
      {runs.map((run, idx) => (
        <Fragment key={idx}>
          {run.matched ? (
            <mark className="kb-palette-mark">{run.text}</mark>
          ) : (
            run.text
          )}
        </Fragment>
      ))}
    </>
  )
}
