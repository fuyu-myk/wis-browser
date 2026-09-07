import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ChangeEvent,
  type FormEvent,
  type KeyboardEvent,
  type MouseEvent,
  type RefObject,
} from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import { ZoomIndicator } from '@/components/ZoomIndicator'
import type { Tab, TabId, WorkspaceId } from '@/bindings/types'
import { LoadProgressBar } from '@/components/LoadProgressBar'
import { cx } from '@/lib/cx'
import { SearchIcon } from '@/lib/icons'
import { activeEngine, isChromeUrl, NEW_TAB_URL, resolveInput } from '@/lib/url'
import { bookmarkFoldersKey, bookmarksKey } from '@/lib/bookmarkKeys'
import { useBookmarkSync } from '@/hooks/useBookmarkSync'
import { BMPICKER_OPEN_EVENT, BOOKMARK_PAGE_EVENT, lastFolderKey, PIN_CAP } from '@/lib/bookmarkMenu'
import {
  CAPTURE_LOGIN_EVENT,
  VAULTMENU_CLOSE_EVENT,
  VAULTMENU_OPEN_EVENT,
  type CaptureInfo,
} from '@/lib/vaultMenu'
import { BROWSER_STATE_KEY, useBrowserState } from '@/hooks/useBrowserState'
import { WisMark } from '@/components/WisMark'
import { FOCUS_URLBAR_EVENT } from '@/hooks/useKeyActions'
import { useOpenTabActions } from '@/hooks/useOpenTabActions'
import { useOverlay } from '@/hooks/useOverlay'
import { useUrlbarSuggest } from '@/hooks/useUrlbarSuggest'

/** Hide the http(s):// prefix in the address bar. */
function displayUrl(url: string): string {
  return url.replace(/^https?:\/\//, '')
}

/** The http(s) host of a tab's URL, or null for chrome/blank pages. */
function hostOf(tab: Tab | null): string | null {
  if (!tab || isChromeUrl(tab.url)) return null
  try {
    const u = new URL(tab.url)
    if (u.protocol !== 'http:' && u.protocol !== 'https:') return null
    return u.host.replace(/^www\./, '')
  } catch {
    return null
  }
}

const VAULT_HAS_KEY = 'vault-has'

/**
 * Key button in the address bar, shown when a login is saved for the current
 * site or a login was just captured there. It anchors the `overlay-vaultmenu`
 * dropdown (the save prompt + saved-account picker) directly beneath itself, so
 * the menu always drops below the header and never covers the tab strip. The
 * chrome owns positioning + open/close; the surface renders + reports height.
 */
function AutofillKey({ activeTab }: { activeTab: Tab | null }) {
  const host = useMemo(() => hostOf(activeTab), [activeTab])
  const qc = useQueryClient()
  const overlay = useOverlay('vaultmenu')
  const btnRef = useRef<HTMLButtonElement>(null)
  const openRef = useRef(false)
  const autoOpenRef = useRef(false)
  const [pendingHost, setPendingHost] = useState<string | null>(null)

  const { data: hasCreds } = useQuery({
    queryKey: [VAULT_HAS_KEY, host],
    queryFn: () => (host ? commands.vaultHasCredentials(host) : Promise.resolve(false)),
    enabled: host !== null,
    staleTime: 30_000,
  })

  // A pending capture keeps the prompt alive regardless of the current page host.
  const hasPending = pendingHost !== null
  const visible = hasPending || (host != null && hasCreds === true)

  const anchorRect = useCallback(() => {
    const el = btnRef.current
    if (!el) return null
    const r = el.getBoundingClientRect()
    const width = 300
    return {
      x: Math.round(Math.max(8, r.right - width)),
      y: Math.round(r.bottom + 6),
      width,
    }
  }, [])

  const open = useCallback(async () => {
    const a = anchorRect()
    if (!a) return
    await overlay
      .show({ x: 0, y: 0, width: window.innerWidth, height: window.innerHeight })
      .catch(() => undefined)
    openRef.current = true
    await emit(VAULTMENU_OPEN_EVENT, a).catch(() => undefined)
  }, [anchorRect, overlay])

  const close = useCallback(async () => {
    if (openRef.current) {
      openRef.current = false
      await overlay.hide().catch(() => undefined)
    }
    setPendingHost(null)
    void qc.invalidateQueries({ queryKey: [VAULT_HAS_KEY] })
  }, [overlay, qc])

  useEffect(() => {
    void commands.ensureOverlay('vaultmenu').catch(() => undefined)
  }, [])

  // Recover an outstanding capture (e.g. after a post-login navigation re-renders
  // this bar).
  useEffect(() => {
    let cancelled = false
    void commands.capturePending().then((p) => {
      if (!cancelled && p) {
        setPendingHost(p.host)
        if (!openRef.current) autoOpenRef.current = true
      }
    })
    return () => {
      cancelled = true
    }
  }, [host])

  useEffect(() => {
    let un: UnlistenFn | null = null
    let cancelled = false
    void listen<CaptureInfo>(CAPTURE_LOGIN_EVENT, (e) => {
      setPendingHost(e.payload.host)
      autoOpenRef.current = true
    }).then((fn) => (cancelled ? fn() : (un = fn)))
    return () => {
      cancelled = true
      un?.()
    }
  }, [])

  useEffect(() => {
    if (visible && autoOpenRef.current) {
      autoOpenRef.current = false
      void open()
    }
  }, [visible, open])

  useEffect(() => {
    let unC: UnlistenFn | null = null
    let cancelled = false
    void listen<void>(VAULTMENU_CLOSE_EVENT, () => void close()).then((fn) =>
      cancelled ? fn() : (unC = fn),
    )
    return () => {
      cancelled = true
      unC?.()
    }
  }, [close])

  useEffect(() => {
    const onResize = () => {
      if (!openRef.current) return
      const a = anchorRect()
      if (!a) return
      void overlay
        .show({ x: 0, y: 0, width: window.innerWidth, height: window.innerHeight })
        .catch(() => undefined)
      void emit(VAULTMENU_OPEN_EVENT, a).catch(() => undefined)
    }
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [anchorRect, overlay])

  useEffect(() => {
    void close()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeTab?.id])

  if (!visible) return null

  const toggle = () => {
    if (openRef.current) void close()
    else void open()
  }

  return (
    <button
      ref={btnRef}
      type="button"
      className="kb-urlbar-key"
      title="Saved logins"
      aria-label="Saved logins"
      onMouseDown={(e) => {
        e.preventDefault()
        e.stopPropagation()
      }}
      onClick={toggle}
    >
      <KeyGlyph />
    </button>
  )
}

function KeyGlyph() {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <circle cx="7.5" cy="15.5" r="4.5" />
      <path d="M10.7 12.3 19 4" />
      <path d="m16 7 3 3" />
      <path d="m14 9 2 2" />
    </svg>
  )
}

function useUrlBarForm(
  activeTab: Tab | null,
  fallbackWorkspaceId: WorkspaceId | null,
  inputRef: RefObject<HTMLInputElement>,
) {
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

  const workspaceId = activeTab?.workspace_id ?? fallbackWorkspaceId

  const { data: state } = useBrowserState()
  const searchSettings = state?.settings

  /** Navigate (or search) for a raw address-bar string, remembering searches. */
  const go = useCallback(
    (raw: string) => {
      const resolved = resolveInput(raw, searchSettings)
      if (!resolved) return
      if (resolved.isSearch && workspaceId) {
        void commands.recordSearch(raw.trim(), workspaceId).catch(() => undefined)
      }
      if (activeTab) {
        navigate.mutate({ id: activeTab.id, url: resolved.url })
      } else if (workspaceId) {
        create.mutate({ workspaceId, url: resolved.url })
      }
    },
    [activeTab, workspaceId, navigate, create, searchSettings],
  )

  const { findOpenTab, switchToTab } = useOpenTabActions()

  const engineLabelFor = useCallback(
    (query: string) =>
      resolveInput(query, searchSettings)?.engineLabel ?? activeEngine(searchSettings).label,
    [searchSettings],
  )

  const suggest = useUrlbarSuggest({
    inputRef,
    workspaceId,
    tabId: activeTab?.id ?? null,
    commit: go,
    setValue,
    switchToTab,
    findOpenTab,
    engineLabelFor,
  })

  function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault()
    suggest.reset()
    go(value)
  }

  function onChange(e: ChangeEvent<HTMLInputElement>) {
    suggest.onInput(e.target.value)
  }

  // Uncommitted edits never outlive the editing session.
  const revert = useCallback(() => {
    setValue(displayUrl(activeTab?.url ?? ''))
  }, [activeTab?.url])

  return { value, onChange, onSubmit, suggest, revert }
}

/**
 * Focus + select-all the URL bar input on receipt of the `wis-focus-urlbar`
 * window event (Cmd+L / Cmd+E). Mounted by each visible URL-bar variant;
 * if both are mounted simultaneously the first one wins.
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

/**
 * Leading URL-bar icon that reflects the active tab's connection security:
 * a padlock for HTTPS, a "not secure" mark for plain HTTP, and the neutral
 * search glyph for chrome pages / the launchpad (where the bar is a search box).
 */
function SecurityIndicator({ activeTab }: { activeTab: Tab | null }) {
  const url = activeTab?.url ?? ''
  if (!url || url === NEW_TAB_URL) return <SearchIcon className="ico" />
  if (isChromeUrl(url)) {
    return (
      <span className="ico kb-chrome-page" title="Wis page" aria-label="Wis page">
        <WisMark variant="solid-closed" size={14} />
      </span>
    )
  }

  let secure = false
  try {
    secure = new URL(url).protocol === 'https:'
  } catch {
    /* not parseable → treat as not secure */
  }

  if (secure) {
    return (
      <span className="ico kb-secure" title="Connection is secure (HTTPS)" aria-label="Secure connection">
        <LockGlyph />
      </span>
    )
  }
  return (
    <span
      className="ico kb-insecure"
      title="Not secure — this site is loaded over HTTP"
      aria-label="Connection is not secure"
    >
      <NotSecureGlyph />
    </span>
  )
}

function LockGlyph() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <rect x="5" y="11" width="14" height="9" rx="2" />
      <path d="M8 11V8a4 4 0 0 1 8 0v3" />
    </svg>
  )
}

function NotSecureGlyph() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M10.3 3.8 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.8a2 2 0 0 0-3.4 0Z" />
      <path d="M12 9v4" />
      <path d="M12 17h.01" />
    </svg>
  )
}

/**
 * Bookmark star on the right of the address bar. Clicking bookmarks the active
 * page into the last-used folder (default: unfiled) and opens a small folder
 * picker anchored under the star to confirm/change it; the choice persists for
 * next time. Clicking again when already bookmarked removes it. Hidden for
 * chrome pages and private (ephemeral) workspaces.
 */
function BookmarkStar({ activeTab }: { activeTab: Tab | null }) {
  const qc = useQueryClient()
  const { data: state } = useBrowserState()
  useBookmarkSync()
  const overlay = useOverlay('bmpicker')
  const btnRef = useRef<HTMLButtonElement>(null)
  const wsId = activeTab?.workspace_id ?? null
  const ephemeral =
    (wsId ? state?.workspaces.find((w) => w.id === wsId)?.ephemeral : false) ?? false
  const bookmarkable = !!activeTab && !isChromeUrl(activeTab.url) && !ephemeral && wsId !== null

  useEffect(() => {
    void commands.ensureOverlay('bmpicker').catch(() => undefined)
  }, [])

  const bookmarks = useQuery({
    queryKey: wsId ? bookmarksKey(wsId) : ['bookmarks', 'none'],
    queryFn: () => commands.listBookmarks(wsId!),
    enabled: bookmarkable,
  })
  const folders = useQuery({
    queryKey: wsId ? bookmarkFoldersKey(wsId) : ['bookmark-folders', 'none'],
    queryFn: () => commands.listBookmarkFolders(wsId!),
    enabled: bookmarkable,
  })
  const existing =
    activeTab && bookmarkable
      ? (bookmarks.data?.find((b) => b.url === activeTab.url) ?? null)
      : null

  const onClick = async () => {
    if (!activeTab || !wsId) return
    if (existing) {
      await commands.deleteBookmark(existing.id).catch(() => undefined)
      void qc.invalidateQueries({ queryKey: bookmarksKey(wsId) })
      return
    }

    // New bookmarks land in the last-used folder (unfiled by default).
    let lastFolderId: number | null = null
    try {
      const raw = localStorage.getItem(lastFolderKey(wsId))
      if (raw && raw !== 'unfiled') {
        const id = Number(raw)
        if ((folders.data ?? []).some((f) => f.id === id)) lastFolderId = id
      }
    } catch {
      /* ignore */
    }

    const pinnedCount = (bookmarks.data ?? []).filter((b) => b.pinned).length
    const autoPin = pinnedCount < PIN_CAP

    const bm = await commands
      .addBookmark({
        workspaceId: wsId,
        folderId: lastFolderId,
        url: activeTab.url,
        title: activeTab.title,
        faviconHash: activeTab.favicon?.hash ?? null,
        pinned: autoPin,
      })
      .catch(() => null)
    void qc.invalidateQueries({ queryKey: bookmarksKey(wsId) })
    if (!bm) return

    // Open the folder picker anchored under the star.
    const el = btnRef.current
    if (!el) return
    const r = el.getBoundingClientRect()
    const width = 240
    await overlay
      .show({ x: 0, y: 0, width: window.innerWidth, height: window.innerHeight })
      .catch(() => undefined)
    await emit(BMPICKER_OPEN_EVENT, {
      bookmarkId: bm.id,
      workspaceId: wsId,
      currentFolderId: lastFolderId,
      x: Math.round(Math.max(8, r.right - width)),
      y: Math.round(r.bottom + 6),
      width,
    }).catch(() => undefined)
  }

  // The bookmark_page keybinding pushes this button. Re-registered per
  // render so the handler always closes over the current bookmark state.
  useEffect(() => {
    if (!bookmarkable) return
    const handler = () => void onClick()
    window.addEventListener(BOOKMARK_PAGE_EVENT, handler)
    return () => window.removeEventListener(BOOKMARK_PAGE_EVENT, handler)
  })

  if (!bookmarkable) return null

  return (
    <button
      ref={btnRef}
      type="button"
      className={cx('kb-urlbar-key', 'kb-urlbar-star', { 'is-on': !!existing })}
      title={existing ? 'Remove bookmark' : 'Bookmark this page'}
      aria-label={existing ? 'Remove bookmark' : 'Bookmark this page'}
      aria-pressed={!!existing}
      onMouseDown={(e) => {
        e.preventDefault()
        e.stopPropagation()
      }}
      onClick={() => void onClick()}
    >
      <StarGlyph filled={!!existing} />
    </button>
  )
}

function StarGlyph({ filled }: { filled: boolean }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill={filled ? 'currentColor' : 'none'}
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="M12 17.3 6.2 20.5l1.1-6.5L2.5 9.3l6.5-.9L12 2.5l3 5.9 6.5.9-4.8 4.7 1.1 6.5z" />
    </svg>
  )
}

/**
 * Hands the current page off to the OS default browser. An exit hatch for
 * pages the embedded WebKit engine can't fully handle (e.g. Widevine video).
 * Shown for any real http(s) page; hidden on chrome pages and the launchpad.
 */
function OpenExternalButton({ activeTab }: { activeTab: Tab | null }) {
  const canOpen = hostOf(activeTab) !== null
  if (!canOpen || !activeTab) return null
  return (
    <button
      type="button"
      className="kb-urlbar-key"
      title="Open in your default browser"
      aria-label="Open in your default browser"
      onMouseDown={(e) => {
        e.preventDefault()
        e.stopPropagation()
      }}
      onClick={() => void commands.openInExternalBrowser(activeTab.url).catch(() => undefined)}
    >
      <ExternalGlyph />
    </button>
  )
}

function ExternalGlyph() {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="M15 3h6v6" />
      <path d="M10 14 21 3" />
      <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
    </svg>
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
  const inputRef = useRef<HTMLInputElement | null>(null)
  const form = useUrlBarForm(activeTab, fallbackWorkspaceId, inputRef)
  const handleMouseDown = useSelectAllOnChromeClick(inputRef)
  useFocusOnEvent(inputRef)
  // First Esc closes the dropdown (handled by the suggest hook); a second
  // discards the edit and blurs.
  const handleKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (form.suggest.onKeyDown(e)) return
    if (e.key === 'Escape') {
      form.revert()
      e.currentTarget.blur()
    }
  }
  const handleBlur = () => {
    form.suggest.onBlur()
    form.revert()
  }
  return (
    <form
      className={cx('kb-urlpill-inline', { inline })}
      aria-label="Address bar"
      onMouseDown={handleMouseDown}
      onSubmit={form.onSubmit}
    >
      <SecurityIndicator activeTab={activeTab} />
      <input
        ref={inputRef}
        className="kb-urlbar-input"
        type="text"
        value={form.value}
        onChange={form.onChange}
        onFocus={form.suggest.onFocus}
        onBlur={handleBlur}
        onKeyDown={handleKeyDown}
        placeholder="Enter search or web address"
        spellCheck={false}
      />
      <ZoomIndicator activeTab={activeTab} />
      <AutofillKey activeTab={activeTab} />
      <OpenExternalButton activeTab={activeTab} />
      <BookmarkStar activeTab={activeTab} />
      <LoadProgressBar activeTab={activeTab} />
    </form>
  )
}
