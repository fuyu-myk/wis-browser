/**
 * Chrome-side controller for the address-bar suggestions dropdown.
 *
 * This hook is the "brain": it fetches suggestions, computes inline completion
 * (filling the bar with the best matching site and selecting the un-typed tail),
 * assembles the grouped row list, owns the keyboard highlight, positions/shows
 * the `overlay-urlbar` webview under the bar, and applies picks/removes. 
 * The overlay surface only renders what this pushes.
 *
 * Focus model: showing the overlay does not steal keyboard focus, so the input
 * keeps it and arrow/Tab/Enter stay live. A mouse click on the overlay does
 * move OS first-responder. Fine for picks (navigation/switch follows),
 * focus is reclaimed on removes so the user can keep typing.
 */

import { useCallback, useEffect, useLayoutEffect, useRef } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { KeyboardEvent, RefObject } from 'react'

import { commands } from '@/bindings/commands'
import type { SiteSuggestion, TabId, WorkspaceId } from '@/bindings/types'
import { isSearchQuery } from '@/lib/url'
import { useOverlay } from '@/hooks/useOverlay'
import {
  estimateHeight,
  isRemovable,
  SECTION_RECENT_SEARCHES,
  SECTION_RECOMMENDATIONS,
  SECTION_TOP,
  URLBAR_PICK_EVENT,
  URLBAR_REMOVE_EVENT,
  URLBAR_RENDER_EVENT,
  type RenderRow,
  type UrlbarIndexPayload,
} from '@/features/urlbar/urlbarSuggest'

/** Gap between the address-bar pill's bottom edge and the dropdown. */
const GAP = 8
/** Delay before a blur closes the dropdown, so an overlay click can cancel it. */
const BLUR_CLOSE_MS = 200

// The overlay-urlbar webview is a per-window singleton shared by every hook
// instance (top bar, launchpad). Exactly one instance owns it at a time.
let overlayOwner: symbol | null = null
const overlayDeactivators = new Map<symbol, () => void>()

function claimOverlay(id: symbol): void {
  if (overlayOwner !== null && overlayOwner !== id) {
    overlayDeactivators.get(overlayOwner)?.()
  }
  overlayOwner = id
}

/** True when `id` owned the overlay (caller may hide it). */
function releaseOverlay(id: symbol): boolean {
  if (overlayOwner !== id) return false
  overlayOwner = null
  return true
}

// Per-section caps.
const EMPTY_SITES = 5
const EMPTY_SEARCHES = 4
const TYPING_SEARCHES = 4
const TYPING_RECS = 3

interface UseUrlbarSuggestArgs {
  inputRef: RefObject<HTMLInputElement>
  workspaceId: WorkspaceId | null
  /** Active tab id; the dropdown closes when this changes (tab switch). */
  tabId: TabId | null
  /** Navigate/search a raw address-bar string (search recording included). */
  commit: (raw: string) => void
  /** Set the controlled input value (used for inline completion). */
  setValue: (value: string) => void
  /** Switch focus to an already-open tab. */
  switchToTab: (tabId: TabId) => void
  /** Resolve a URL to an open tab id, or null if not open. */
  findOpenTab: (url: string) => TabId | null
  /**
   * Engine label for the "Search with X" row, resolved per query.
   */
  engineLabelFor: (query: string) => string
}

export interface UrlbarSuggestApi {
  onFocus: () => void
  onInput: (next: string) => void
  /** Returns true when the key was consumed by the dropdown. */
  onKeyDown: (e: KeyboardEvent<HTMLInputElement>) => boolean
  onBlur: () => void
  close: () => void
  /** Close + forget in-progress text (used by the form on submit). */
  reset: () => void
}

export function useUrlbarSuggest(args: UseUrlbarSuggestArgs): UrlbarSuggestApi {
  const { inputRef, workspaceId, tabId } = args
  const overlay = useOverlay('urlbar')

  const idRef = useRef<symbol | null>(null)
  if (idRef.current === null) idRef.current = Symbol('urlbar-suggest')

  const argsRef = useRef(args)
  argsRef.current = args

  const rowsRef = useRef<RenderRow[]>([])
  const highlightRef = useRef(-1)
  const openRef = useRef(false)
  const queryRef = useRef('')
  const reqIdRef = useRef(0)
  const blurTimerRef = useRef<number | null>(null)
  const isDeletingRef = useRef(false)
  const pendingSelRef = useRef<{ start: number; end: number } | null>(null)
  // The user's actual typed text (sans inline completion). Preserved across
  // blur so refocus can restore both the text and its dropdown/completion.
  const pendingTypedRef = useRef('')

  // Apply a pending inline-completion selection after the value renders.
  useLayoutEffect(() => {
    const sel = pendingSelRef.current
    if (!sel) return
    pendingSelRef.current = null
    const el = inputRef.current
    if (!el) return
    try {
      el.setSelectionRange(sel.start, sel.end)
    } catch {
      /* selection not supported on this input state */
    }
  })

  const pushRender = useCallback(() => {
    void emit(URLBAR_RENDER_EVENT, {
      rows: rowsRef.current,
      highlight: highlightRef.current,
    }).catch(() => undefined)
  }, [])

  const close = useCallback(() => {
    reqIdRef.current += 1
    if (!openRef.current) return
    openRef.current = false
    highlightRef.current = -1
    rowsRef.current = []
    if (releaseOverlay(idRef.current as symbol)) {
      void overlay.hide().catch(() => undefined)
    }
  }, [overlay])

  /** Close and forget the in-progress typed text (after a commit / tab switch). */
  const reset = useCallback(() => {
    pendingTypedRef.current = ''
    close()
  }, [close])

  const showUnderBar = useCallback(async () => {
    const el = inputRef.current
    if (!el) return
    claimOverlay(idRef.current as symbol)
    const pill = (el.closest('form') as HTMLElement | null) ?? el
    const r = pill.getBoundingClientRect()
    await overlay
      .show({
        x: Math.round(r.left),
        y: Math.round(r.bottom + GAP),
        width: Math.round(r.width),
        height: estimateHeight(rowsRef.current),
      })
      .catch(() => undefined)
    openRef.current = true
  }, [inputRef, overlay])

  /** Build the grouped row list for a given query + fetched suggestions. */
  const assemble = useCallback(
    (
      query: string,
      searches: { query: string }[],
      sites: SiteSuggestion[],
      completion: SiteSuggestion | null,
    ): RenderRow[] => {
      const { findOpenTab, engineLabelFor } = argsRef.current
      const siteRow = (s: SiteSuggestion, section: string): RenderRow => ({
        kind: 'site',
        section,
        id: s.id,
        url: s.url,
        title: s.title,
        openTabId: findOpenTab(s.url),
      })
      const rows: RenderRow[] = []

      if (query === '') {
        // Untitled top block of most-visited sites, then recent searches.
        for (const s of sites.slice(0, EMPTY_SITES)) rows.push(siteRow(s, SECTION_TOP))
        for (const s of searches.slice(0, EMPTY_SEARCHES)) {
          rows.push({ kind: 'search', section: SECTION_RECENT_SEARCHES, query: s.query })
        }
        return rows
      }

      const q = query.toLowerCase()
      // Inline-completion target (the autocomplete site).
      if (completion) rows.push(siteRow(completion, SECTION_TOP))
      // Matching past searches.
      for (const s of searches.filter((x) => x.query.toLowerCase() !== q).slice(0, TYPING_SEARCHES)) {
        rows.push({ kind: 'search', section: SECTION_TOP, query: s.query })
      }
      // "Search <engine> for X", only when the input is a search, not a URL.
      if (isSearchQuery(query)) {
        rows.push({ kind: 'searchQuery', section: SECTION_TOP, query, engineLabel: engineLabelFor(query) })
      }
      // Recommendations pinned below: close matches, excluding the completion.
      const recs = sites.filter((s) => !completion || s.url !== completion.url).slice(0, TYPING_RECS)
      for (const s of recs) rows.push(siteRow(s, SECTION_RECOMMENDATIONS))
      return rows
    },
    [],
  )

  /**
   * The base website to inline-complete to: the first visited site whose host
   * prefixes the query, reduced to its origin ("youtube.com/", never a deep
   * link). Returns a ready site row + the display string for the bar.
   */
  const findCompletion = useCallback(
    (sites: SiteSuggestion[], query: string): { row: SiteSuggestion; display: string } | null => {
      const q = query.toLowerCase()
      for (const s of sites) {
        let host: string
        let baseUrl: string
        try {
          const u = new URL(s.url)
          host = u.host.replace(/^www\./, '')
          baseUrl = `${u.protocol}//${host}/`
        } catch {
          continue
        }
        const display = `${host}/`
        if (display.toLowerCase().startsWith(q)) {
          return { row: { id: s.id, url: baseUrl, title: '' }, display }
        }
      }
      return null
    },
    [],
  )

  const refresh = useCallback(
    async (rawQuery: string, autocomplete: boolean) => {
      const ws = workspaceId
      if (!ws) return
      const query = rawQuery.trim()
      queryRef.current = query
      const reqId = ++reqIdRef.current
      const res = await commands.getUrlSuggestions(query, ws).catch(() => null)
      if (reqId !== reqIdRef.current) return // superseded by a newer keystroke
      if (!res) return

      const completion = autocomplete && rawQuery ? findCompletion(res.sites, rawQuery) : null
      if (completion && completion.display.toLowerCase() !== rawQuery.toLowerCase()) {
        argsRef.current.setValue(completion.display)
        pendingSelRef.current = { start: rawQuery.length, end: completion.display.length }
      }

      const rows = assemble(query, res.searches, res.sites, completion?.row ?? null)
      rowsRef.current = rows
      if (highlightRef.current >= rows.length) highlightRef.current = -1
      if (rows.length === 0) {
        close()
        return
      }
      await showUnderBar()
      if (reqId !== reqIdRef.current) {
        openRef.current = false
        highlightRef.current = -1
        rowsRef.current = []
        if (releaseOverlay(idRef.current as symbol)) {
          void overlay.hide().catch(() => undefined)
        }
        return
      }
      pushRender()
    },
    [workspaceId, assemble, findCompletion, close, showUnderBar, pushRender, overlay],
  )

  const move = useCallback(
    (delta: number) => {
      const n = rowsRef.current.length
      if (n === 0) return
      let h = highlightRef.current + delta
      if (h < -1) h = n - 1
      else if (h >= n) h = -1
      highlightRef.current = h
      pushRender()
    },
    [pushRender],
  )

  const pickRow = useCallback(
    (index: number) => {
      const row = rowsRef.current[index]
      reset()
      if (!row) return
      if (row.kind === 'site') {
        if (row.openTabId) argsRef.current.switchToTab(row.openTabId)
        else argsRef.current.commit(row.url)
      } else {
        argsRef.current.commit(row.query)
      }
    },
    [reset],
  )

  const cancelBlur = useCallback(() => {
    if (blurTimerRef.current != null) {
      clearTimeout(blurTimerRef.current)
      blurTimerRef.current = null
    }
  }, [])

  const removeRow = useCallback(
    async (index: number) => {
      const row = rowsRef.current[index]
      if (!row || !isRemovable(row)) return
      if (row.kind === 'search') {
        await commands.deleteSearch(row.query).catch(() => undefined)
      } else if (row.kind === 'site') {
        await commands.deleteHistory([row.id]).catch(() => undefined)
      }
      await commands.focusSelf().catch(() => undefined)
      inputRef.current?.focus()
      await refresh(queryRef.current, false)
    },
    [inputRef, refresh],
  )

  // Register this instance's silent deactivator: when another instance
  // claims the overlay, drop all open state here without hiding (the winner
  // is about to re-show it) so pick broadcasts and delayed closes no-op.
  const hideOverlay = overlay.hide
  useEffect(() => {
    const id = idRef.current as symbol
    overlayDeactivators.set(id, () => {
      reqIdRef.current += 1
      openRef.current = false
      highlightRef.current = -1
      rowsRef.current = []
      cancelBlur()
    })
    return () => {
      overlayDeactivators.delete(id)
      // Unmounting while owning the overlay must hide it.
      if (releaseOverlay(id)) {
        void hideOverlay().catch(() => undefined)
      }
    }
  }, [cancelBlur, hideOverlay])

  // Overlay → chrome: row click / ✕ click.
  useEffect(() => {
    let unPick: UnlistenFn | null = null
    let unRemove: UnlistenFn | null = null
    let cancelled = false
    void listen<UrlbarIndexPayload>(URLBAR_PICK_EVENT, (e) => {
      cancelBlur()
      pickRow(e.payload.index)
    }).then((fn) => (cancelled ? fn() : (unPick = fn)))
    void listen<UrlbarIndexPayload>(URLBAR_REMOVE_EVENT, (e) => {
      cancelBlur()
      void removeRow(e.payload.index)
    }).then((fn) => (cancelled ? fn() : (unRemove = fn)))
    return () => {
      cancelled = true
      unPick?.()
      unRemove?.()
    }
  }, [cancelBlur, pickRow, removeRow])

  useEffect(() => {
    const onResize = () => {
      if (openRef.current) void showUnderBar()
    }
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [showUnderBar])

  // Forget in-progress text + close on tab switch.
  useEffect(() => {
    reset()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tabId])

  useEffect(() => () => cancelBlur(), [cancelBlur])

  const onFocus = useCallback(() => {
    cancelBlur()
    // Restore an in-progress query (and its inline completion) if the user
    // left and came back; otherwise show the empty dropdown.
    const pending = pendingTypedRef.current
    if (pending) void refresh(pending, true)
    else void refresh('', false)
  }, [cancelBlur, refresh])

  const onInput = useCallback(
    (next: string) => {
      const deleting = isDeletingRef.current
      isDeletingRef.current = false
      pendingTypedRef.current = next
      argsRef.current.setValue(next)
      void refresh(next, !deleting)
    },
    [refresh],
  )

  const onBlur = useCallback(() => {
    cancelBlur()
    // Drop the inline completion: leave only what the user actually typed.
    if (pendingTypedRef.current) argsRef.current.setValue(pendingTypedRef.current)
    blurTimerRef.current = window.setTimeout(() => close(), BLUR_CLOSE_MS)
  }, [cancelBlur, close])

  // Click-away fallbacks.
  useEffect(() => {
    const onWindowBlur = () => {
      if (openRef.current) onBlur()
    }
    const onPointerDown = (e: PointerEvent) => {
      if (!openRef.current) return
      const el = inputRef.current
      const pill = (el?.closest('form') as HTMLElement | null) ?? el
      if (pill && e.target instanceof Node && pill.contains(e.target)) return
      if (pendingTypedRef.current) argsRef.current.setValue(pendingTypedRef.current)
      close()
    }
    window.addEventListener('blur', onWindowBlur)
    document.addEventListener('pointerdown', onPointerDown, true)
    return () => {
      window.removeEventListener('blur', onWindowBlur)
      document.removeEventListener('pointerdown', onPointerDown, true)
    }
  }, [onBlur, close, inputRef])

  const onKeyDown = useCallback(
    (e: KeyboardEvent<HTMLInputElement>): boolean => {
      if (e.key === 'Backspace' || e.key === 'Delete') {
        isDeletingRef.current = true
        return false
      }
      if (!openRef.current) return false
      switch (e.key) {
        case 'ArrowDown':
          e.preventDefault()
          move(1)
          return true
        case 'ArrowUp':
          e.preventDefault()
          move(-1)
          return true
        case 'Tab':
          e.preventDefault()
          move(e.shiftKey ? -1 : 1)
          return true
        case 'Escape':
          e.preventDefault()
          // Drop the inline completion but keep the typed text, then close.
          if (pendingTypedRef.current) argsRef.current.setValue(pendingTypedRef.current)
          close()
          return true
        case 'Enter': {
          const h = highlightRef.current
          if (h >= 0 && h < rowsRef.current.length) {
            e.preventDefault()
            pickRow(h)
            return true
          }
          close() // fall through to the form's typed-text submit
          return false
        }
        default:
          return false
      }
    },
    [close, move, pickRow],
  )

  return { onFocus, onInput, onKeyDown, onBlur, close, reset }
}
