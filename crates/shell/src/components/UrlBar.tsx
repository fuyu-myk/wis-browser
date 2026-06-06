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
import type { Tab, TabId, WorkspaceId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { SearchIcon } from '@/lib/icons'
import { isChromeUrl, isSearchQuery, normaliseUrl, SEARCH_ENGINES } from '@/lib/url'
import {
  CAPTURE_LOGIN_EVENT,
  VAULTMENU_CLOSE_EVENT,
  VAULTMENU_OPEN_EVENT,
  type CaptureInfo,
} from '@/lib/vaultMenu'
import { BROWSER_STATE_KEY, useBrowserState } from '@/hooks/useBrowserState'
import { FOCUS_URLBAR_EVENT } from '@/hooks/useKeyActions'
import { useOverlay } from '@/hooks/useOverlay'
import { useUrlbarSuggest } from '@/hooks/useUrlbarSuggest'

/** Canonical form for matching a suggestion URL against an open tab's URL. */
function canonUrl(url: string): string {
  return url
    .replace(/^https?:\/\//, '')
    .replace(/^www\./, '')
    .replace(/\/$/, '')
    .toLowerCase()
}

/** Hide the http(s):// prefix in the address bar. */
function displayUrl(url: string): string {
  return url.replace(/^https?:\/\//, '')
}

/** The http(s) host of a tab's URL, or null for chrome/blank pages. */
function hostOf(tab: Tab | null): string | null {
  if (!tab || isChromeUrl(tab.url)) return null
  try {
    const u = new URL(tab.url)
    return u.protocol === 'http:' || u.protocol === 'https:' ? u.host : null
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

  const pendingForHost = host != null && pendingHost === host
  const visible = host != null && (hasCreds === true || pendingForHost)

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
    setPendingHost((p) => (p === host ? null : p))
    void qc.invalidateQueries({ queryKey: [VAULT_HAS_KEY] })
  }, [overlay, host, qc])

  useEffect(() => {
    void commands.ensureOverlay('vaultmenu').catch(() => undefined)
  }, [])

  useEffect(() => {
    if (!host) return
    let cancelled = false
    void commands.capturePending().then((p) => {
      if (!cancelled && p && p.host === host) {
        setPendingHost(host)
        autoOpenRef.current = true
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
      if (host && e.payload.host === host) {
        setPendingHost(host)
        autoOpenRef.current = true
      }
    }).then((fn) => (cancelled ? fn() : (un = fn)))
    return () => {
      cancelled = true
      un?.()
    }
  }, [host])

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
  const engine = state?.settings.search_engine ?? 'duckduckgo'
  const engineLabel = SEARCH_ENGINES[engine].label

  /** Navigate (or search) for a raw address-bar string, remembering searches. */
  const go = useCallback(
    (raw: string) => {
      const target = normaliseUrl(raw, engine)
      if (!target) return
      if (isSearchQuery(raw)) {
        void commands.recordSearch(raw.trim()).catch(() => undefined)
      }
      if (activeTab) {
        navigate.mutate({ id: activeTab.id, url: target })
      } else if (workspaceId) {
        create.mutate({ workspaceId, url: target })
      }
    },
    [activeTab, workspaceId, navigate, create, engine],
  )

  /** Resolve a suggestion URL to an already-open tab id, or null. */
  const findOpenTab = useCallback(
    (url: string): TabId | null => {
      const target = canonUrl(url)
      return state?.tabs.find((t) => canonUrl(t.url) === target)?.id ?? null
    },
    [state?.tabs],
  )

  const switchToTab = useCallback(
    (id: TabId) => {
      const tab = state?.tabs.find((t) => t.id === id)
      if (!tab) return
      const run = async () => {
        if (tab.workspace_id !== state?.active_workspace_id) {
          await commands.setActiveWorkspace(tab.workspace_id).catch(() => undefined)
        }
        await commands.setActiveTab(id).catch(() => undefined)
        void qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY })
      }
      void run()
    },
    [state?.tabs, state?.active_workspace_id, qc],
  )

  const suggest = useUrlbarSuggest({
    inputRef,
    workspaceId,
    tabId: activeTab?.id ?? null,
    commit: go,
    setValue,
    switchToTab,
    findOpenTab,
    engineLabel,
  })

  function onSubmit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault()
    suggest.reset()
    go(value)
  }

  function onChange(e: ChangeEvent<HTMLInputElement>) {
    suggest.onInput(e.target.value)
  }

  return { value, onChange, onSubmit, suggest }
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
  if (!url || isChromeUrl(url)) return <SearchIcon className="ico" />

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
  const handleKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (form.suggest.onKeyDown(e)) return
    if (e.key === 'Escape') e.currentTarget.blur()
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
        onBlur={form.suggest.onBlur}
        onKeyDown={handleKeyDown}
        placeholder="Enter search or web address"
        spellCheck={false}
      />
      <AutofillKey activeTab={activeTab} />
    </form>
  )
}
