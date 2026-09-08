/**
 * Opens the page context menu when a tab reports a right-click.
 *
 * The page reports the pointer in its own CSS pixels. The scale is measured:
 * the chrome already knows the pane's rect, so `pane.width` over the page's
 * `innerWidth` is the factor, whatever the zoom happens to be.
 *
 * The overlay is sized to the menu card. A full-window overlay would swallow
 * the next right-click. Sized to the card, a right-click anywhere else reaches
 * the page (or the chrome) and re-opens the menu there.
 *
 * Dismissal therefore needs the three signals the tab menu uses: Escape inside
 * the surface, a left press in the chrome, and `wis-pane-focus` for a left
 * press that landed inside a page webview.
 */

import { useCallback, useEffect, useRef } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { commands } from '@/bindings/commands'
import type { BrowserState, TabId } from '@/bindings/types'
import { useBrowserState } from '@/hooks/useBrowserState'
import {
  MENU_LABEL_ATTR,
  MENU_URL_ATTR,
  PAGE_MENU_ACK_EVENT,
  PAGE_MENU_DONE_EVENT,
  PAGE_MENU_OPEN_EVENT,
  PAGE_MENU_READY_EVENT,
  type PageMenuReport,
  type PageMenuTarget,
} from '@/lib/pageMenu'
import {
  clearSelectionPaint,
  MAX_SELECTION_CHARS,
  paintSelection,
} from '@/lib/selectionPaint'
import { isWindowFocused } from '@/lib/windowFocus'

/** The page reports a right-click in a tab webview. */
const PAGE_MENU_REPORT_EVENT = 'wis-page-menu'
/** How long the surface has to report that it drew before the overlay goes. */
const ACK_GRACE_MS = 1500
/** A left press inside a page webview. */
const PANE_FOCUS_EVENT = 'wis-pane-focus'
/** Card width. */
const MENU_WIDTH = 232
/** Card's initial height; the surface corrects once it has measured */
const MENU_INITIAL_HEIGHT = 420
/** Kept off the window edges. */
const MARGIN = 8

/**
 * What a link would look like with its tracking parameters gone. One round
 * trip per menu, only when a link was hit, and a failure never blocks the
 * menu.
 */
async function cleanOf(href: string | null): Promise<{ url: string | null; removed: string[] }> {
  if (!href) return { url: null, removed: [] }
  const cleaned = await commands.cleanLink(href).catch(() => null)
  if (!cleaned || cleaned.removed.length === 0) return { url: null, removed: [] }
  return { url: cleaned.url, removed: cleaned.removed }
}
/** How long a press inside a page waits before it dismisses. */
const PANE_DISMISS_DELAY_MS = 120

export function usePageMenu(): void {
  const { data: state } = useBrowserState()
  const stateRef = useRef<BrowserState | undefined>(state)
  stateRef.current = state
  const pendingRef = useRef<PageMenuTarget | null>(null)
  const openRef = useRef(false)
  const seqRef = useRef(0)
  const ackRef = useRef(0)
  const dismissTimerRef = useRef<number | null>(null)
  /** The tab whose page is showing a stand-in highlight, if any. */
  const paintedTabRef = useRef<TabId | null>(null)

  /**
   * Drop both stand-in highlights.
   *
   * Every way the menu can close has to call this, and the menu closes two
   * ways: the chrome dismissing it, and the surface reporting that it closed
   * itself after a row ran or Escape was pressed.
   */
  const releasePaint = useCallback(() => {
    clearSelectionPaint()
    const painted = paintedTabRef.current
    paintedTabRef.current = null
    if (painted) void commands.clearMenuSelection(painted).catch(() => undefined)
  }, [])

  const dismiss = useCallback(async () => {
    if (!openRef.current) return
    openRef.current = false
    pendingRef.current = null
    releasePaint()
    await commands.hideOverlay('pagemenu').catch(() => undefined)
  }, [releasePaint])

  const openTarget = useCallback(
    async (target: PageMenuTarget) => {
      if (dismissTimerRef.current !== null) {
        window.clearTimeout(dismissTimerRef.current)
        dismissTimerRef.current = null
      }
      pendingRef.current = target
      openRef.current = true
      await emit(PAGE_MENU_OPEN_EVENT, target).catch(() => undefined)

      // Safety net for a surface that fails to draw.
      const seq = target.seq
      window.setTimeout(() => {
        if (openRef.current && ackRef.current < seq) void dismiss()
      }, ACK_GRACE_MS)
    },
    [dismiss],
  )

  const open = useCallback(
    async (report: PageMenuReport) => {
      const browser = stateRef.current
      if (!browser) return

      const tabId = report.tabId as TabId
      const dropPaint = () => {
        if (report.selection) void commands.clearMenuSelection(tabId).catch(() => undefined)
      }

      const tab = browser.tabs.find((candidate) => candidate.id === tabId)
      if (!tab) {
        dropPaint()
        return
      }

      // The pane hosting this tab's webview, in the coordinate space
      // `setOverlayBounds` consumes.
      const pane = document
        .querySelector(`[data-pane-leaf="${CSS.escape(report.tabId)}"]`)
        ?.getBoundingClientRect()
      if (!pane) {
        dropPaint()
        return
      }

      // Whether the clicked pane is already the one that acts.
      const workspace = browser.workspaces.find((w) => w.id === tab.workspace_id)

      const scale = report.viewWidth > 0 ? pane.width / report.viewWidth : 1
      const x = pane.x + report.x * scale
      const y = pane.y + report.y * scale
      const host = document.querySelector('.kb-content-host')?.getBoundingClientRect()

      const clean = await cleanOf(report.linkHref)

      paintedTabRef.current = report.selection ? tabId : null

      seqRef.current += 1
      const target: PageMenuTarget = {
        kind: 'page',
        seq: seqRef.current,
        isActiveTab: workspace?.active_tab_id === tabId,
        window: getCurrentWindow().label,
        tabId,
        workspaceId: tab.workspace_id,
        x,
        y,
        winW: window.innerWidth,
        winH: window.innerHeight,
        peekX: host ? x - host.left : -1,
        peekY: host ? y - host.top : -1,
        linkHref: report.linkHref,
        linkText: report.linkText,
        imageSrc: report.imageSrc,
        mediaKind: report.mediaKind,
        mediaSrc: report.mediaSrc,
        selection: report.selection,
        canGoBack: tab.can_go_back,
        canGoForward: tab.can_go_forward,
        cleanLink: clean.url,
        cleanRemoved: clean.removed,
        ephemeral: workspace?.ephemeral ?? false,
      }

      await openTarget(target)
    },
    [openTarget],
  )

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<PageMenuReport>(PAGE_MENU_REPORT_EVENT, (event) => {
      if (!isWindowFocused()) return
      void open(event.payload)
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [open])

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<void>(PAGE_MENU_READY_EVENT, () => {
      if (!isWindowFocused()) return
      const pending = pendingRef.current
      if (!pending) return
      pendingRef.current = null
      void emit(PAGE_MENU_OPEN_EVENT, pending).catch(() => undefined)
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<void>(PAGE_MENU_DONE_EVENT, () => {
      openRef.current = false
      pendingRef.current = null
      releasePaint()
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [releasePaint])

  useEffect(() => {
    const onContextMenu = (event: MouseEvent) => {
      if (event.defaultPrevented) return
      const node = event.target instanceof HTMLElement ? event.target : null
      if (!node || node.closest('input, textarea, [contenteditable=""], [contenteditable="true"]')) {
        return
      }

      const browser = stateRef.current
      if (!browser) return
      const workspace = browser.workspaces.find((w) => w.id === browser.active_workspace_id)
      const tabId = workspace?.active_tab_id ?? null
      if (!workspace || !tabId) return

      const anchor = node.closest<HTMLAnchorElement>('a[href]')
      const stated = node.closest<HTMLElement>(`[${MENU_URL_ATTR}]`)
      const linkHref = anchor?.href ?? stated?.getAttribute(MENU_URL_ATTR) ?? null
      const linkText =
        anchor?.textContent?.trim() ||
        stated?.getAttribute(MENU_LABEL_ATTR) ||
        null
      const imageSrc = node.closest('img')?.currentSrc ?? null
      clearSelectionPaint()
      const live = window.getSelection()
      const selection = live?.toString().trim().slice(0, MAX_SELECTION_CHARS) || null

      if (!linkHref && !imageSrc && !selection) return
      event.preventDefault()
      if (selection && live) paintSelection(live)

      const host = document.querySelector('.kb-content-host')?.getBoundingClientRect()
      seqRef.current += 1
      const seq = seqRef.current
      void cleanOf(linkHref).then((clean) =>
        openTarget({
          kind: 'chrome',
          seq,
          isActiveTab: true,
          window: getCurrentWindow().label,
          tabId,
          workspaceId: workspace.id,
          x: event.clientX,
          y: event.clientY,
          winW: window.innerWidth,
          winH: window.innerHeight,
          peekX: host ? event.clientX - host.left : -1,
          peekY: host ? event.clientY - host.top : -1,
          linkHref,
          linkText,
          imageSrc,
          mediaKind: null,
          mediaSrc: null,
          selection,
          canGoBack: false,
          canGoForward: false,
          cleanLink: clean.url,
          cleanRemoved: clean.removed,
          ephemeral: workspace.ephemeral,
        }),
      )
    }
    window.addEventListener('contextmenu', onContextMenu)
    return () => window.removeEventListener('contextmenu', onContextMenu)
  }, [openTarget])

  // The surface reports that it drew, which is what clears the safety net.
  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<number>(PAGE_MENU_ACK_EVENT, (event) => {
      ackRef.current = Math.max(ackRef.current, event.payload)
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  // Close when the OS window loses focus.
  useEffect(() => {
    let unlisten: (() => void) | null = null
    let cancelled = false
    void getCurrentWindow()
      .onFocusChanged(({ payload: hasFocus }) => {
        if (!hasFocus) void dismiss()
      })
      .then((fn) => {
        if (cancelled) fn()
        else unlisten = fn
      })
      .catch(() => undefined)
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [dismiss])

  useEffect(() => {
    void commands.ensureOverlay('pagemenu').catch(() => undefined)
  }, [])

  // A left press in the chrome closes the menu.
  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      if (event.button === 2) return
      void dismiss()
    }
    window.addEventListener('pointerdown', onPointerDown, true)
    return () => window.removeEventListener('pointerdown', onPointerDown, true)
  }, [dismiss])

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<string>(PANE_FOCUS_EVENT, () => {
      if (dismissTimerRef.current !== null) window.clearTimeout(dismissTimerRef.current)
      dismissTimerRef.current = window.setTimeout(() => {
        dismissTimerRef.current = null
        void dismiss()
      }, PANE_DISMISS_DELAY_MS)
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [dismiss])

  useEffect(() => {
    const onResize = () => void dismiss()
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [dismiss])
}
