/**
 * The page context menu, hosted by the `overlay-pagemenu` webview.
 *
 * The overlay is the card, so a right-click anywhere else reaches the page and
 * re-opens the menu there. A full-window overlay would swallow that second
 * click, leaving it able only to dismiss. Dismissal for ordinary clicks is the
 * chrome's job (`usePageMenu`); Escape is handled here.
 *
 * Keyboard handling is bound to the window rather than to the card, because a
 * React `onKeyDown` only fires once DOM focus is already inside the menu. So
 * disabled rows use `aria-disabled`, the cursor starts on the first row that
 * is actually enabled, and focus is moved in a layout effect once the card
 * is placed and visible.
 */

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { commands } from '@/bindings/commands'
import type { AccentColor } from '@/bindings/types'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'
import { cx } from '@/lib/cx'
import { menusGlass } from '@/lib/glass'
import { showRemoteToast } from '@/lib/toast'
import {
  PAGE_MENU_ACK_EVENT,
  PAGE_MENU_DONE_EVENT,
  PAGE_MENU_OPEN_EVENT,
  PAGE_MENU_READY_EVENT,
  type PageMenuTarget,
} from '@/lib/pageMenu'
import { currentPlatform } from '@/lib/platform'
import { useAiAvailable } from '@/hooks/useAiAvailable'
import { nextAccent } from '@/lib/accents'
import { AI_ASK_EVENT } from '@/lib/aiAsk'
import { activeEngine, isWebUrl, sourceUrlFor } from '@/lib/url'

/** Kept off the window edges. */
const MARGIN = 8
const MENU_WIDTH = 232
/** A selection past this is not a search query any engine would accept. */
const MAX_SEARCH_CHARS = 2000
/** Longer than this, an error is reported as its plainer fallback. */
const MAX_TOAST_CHARS = 120
/** Peek is opened by the event its own surface listens for. */
const PEEK_OPEN_EVENT = 'wis-peek-open'

interface Row {
  id: string
  label: string
  disabled?: boolean
  run: () => void | Promise<void>
}

export function PageMenuSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)

  const [target, setTarget] = useState<PageMenuTarget | null>(null)
  const [cursor, setCursor] = useState(0)
  const cardRef = useRef<HTMLDivElement | null>(null)
  const itemsRef = useRef<Array<HTMLButtonElement | null>>([])
  const busyRef = useRef(false)
  const workspacesRef = useRef<Array<{ accent: AccentColor }>>([])
  workspacesRef.current = state?.workspaces ?? []

  const close = useCallback(async () => {
    const tabId = target?.tabId ?? null
    setTarget(null)
    await commands.hideOverlay('pagemenu').catch(() => undefined)
    await emit(PAGE_MENU_DONE_EVENT).catch(() => undefined)
    if (tabId && currentPlatform() !== 'macos') {
      await commands.focusWebview(`tab-${tabId}`).catch(() => undefined)
    }
  }, [target])

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<PageMenuTarget>(PAGE_MENU_OPEN_EVENT, (event) => {
      // Events reach every window's surface.
      if (event.payload.window !== getCurrentWindow().label) {
        void commands.hideOverlay('pagemenu').catch(() => undefined)
        return
      }
      setTarget(event.payload)
      // Tells the chrome this open was taken, so its safety net stands down.
      void emit(PAGE_MENU_ACK_EVENT, event.payload.seq).catch(() => undefined)
      void commands.focusSelf().catch(() => undefined)
    }).then((fn) => {
      if (cancelled) {
        fn()
        return
      }
      unlisten = fn
      // Announced only once the listener is live.
      void emit(PAGE_MENU_READY_EVENT).catch(() => undefined)
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  const rows = useRows(target, close, busyRef, workspacesRef)
  const firstEnabled = useMemo(() => {
    const index = rows.findIndex((row) => !row.disabled)
    return index >= 0 ? index : 0
  }, [rows])

  // A fresh menu starts on a row that can actually be used.
  useEffect(() => {
    setCursor(firstEnabled)
  }, [target, firstEnabled])

  // The overlay itself is moved to fit, once the card's real height is known.
  useLayoutEffect(() => {
    if (!target || !cardRef.current) return
    const height = cardRef.current.offsetHeight
    void commands
      .setOverlayBounds(
        'pagemenu',
        Math.max(MARGIN, Math.min(target.x, target.winW - MENU_WIDTH - MARGIN)),
        Math.max(MARGIN, Math.min(target.y, target.winH - height - MARGIN)),
        MENU_WIDTH,
        height,
      )
      .then(() => commands.promoteOverlay('pagemenu'))
      .catch(() => undefined)
  }, [target, rows.length])

  // Focus follows the cursor, so Escape and the arrow keys reach the menu.
  useLayoutEffect(() => {
    if (!target) return
    itemsRef.current[cursor]?.focus()
  }, [target, cursor])

  useEffect(() => {
    if (!target) return
    document.documentElement.setAttribute('data-keybind-capture', '')
    return () => document.documentElement.removeAttribute('data-keybind-capture')
  }, [target])

  const step = useCallback(
    (from: number, delta: number) => {
      if (rows.length === 0) return
      let next = from
      for (let attempt = 0; attempt < rows.length; attempt += 1) {
        next = (next + delta + rows.length) % rows.length
        if (!rows[next].disabled) {
          setCursor(next)
          return
        }
      }
    },
    [rows],
  )

  useEffect(() => {
    if (!target) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault()
        void close()
      } else if (event.key === 'ArrowDown') {
        event.preventDefault()
        step(cursor, 1)
      } else if (event.key === 'ArrowUp') {
        event.preventDefault()
        step(cursor, -1)
      } else if (event.key === 'Home') {
        event.preventDefault()
        step(-1, 1)
      } else if (event.key === 'End') {
        event.preventDefault()
        step(rows.length, -1)
      } else if (event.key === 'Enter' || event.key === ' ') {
        event.preventDefault()
        const row = rows[cursor]
        if (row && !row.disabled) void row.run()
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [target, rows, cursor, step, close])

  if (!target || rows.length === 0) return null

  return (
    <div
      ref={cardRef}
      className={cx('kb-pagemenu', { 'glass-menus': menusGlass(state?.settings) })}
      role="menu"
      aria-label="Page actions"
      style={{ width: MENU_WIDTH }}
      onContextMenu={(event) => event.preventDefault()}
    >
      {rows.map((row, index) => (
        <button
          key={row.id}
          type="button"
          role="menuitem"
          ref={(element) => {
            itemsRef.current[index] = element
          }}
          className={cx('kb-pagemenu-item', {
            'is-cursor': index === cursor,
            'is-disabled': row.disabled,
          })}
          aria-disabled={row.disabled ? true : undefined}
          tabIndex={index === cursor ? 0 : -1}
          onMouseEnter={() => !row.disabled && setCursor(index)}
          onClick={() => {
            if (!row.disabled) void row.run()
          }}
        >
          {row.label}
        </button>
      ))}
    </div>
  )
}

/**
 * The rows for a target, grouped link, image, media, selection, then page.
 * Every row is one existing command.
 */
function useRows(
  target: PageMenuTarget | null,
  close: () => Promise<void>,
  busyRef: React.MutableRefObject<boolean>,
  workspacesRef: React.MutableRefObject<Array<{ accent: AccentColor }>>,
): Row[] {
  const { data: state } = useBrowserState()
  const settings = state?.settings
  const aiReady = useAiAvailable(settings?.ai)

  return useMemo(() => {
    if (!target) return []

    const rows: Row[] = []
    const after = (work: () => Promise<unknown>) => async () => {
      await close()
      await work().catch(() => undefined)
    }
    const afterHere = (work: () => Promise<unknown>) =>
      after(async () => {
        if (!target.isActiveTab) await commands.setActiveTab(target.tabId).catch(() => undefined)
        await work()
      })
    const afterOnce = (work: () => Promise<unknown>) =>
      after(async () => {
        if (busyRef.current) return
        busyRef.current = true
        try {
          await work()
        } finally {
          busyRef.current = false
        }
      })

    const reportFailure = (fallback: string) => (error: unknown) => {
      const detail =
        typeof error === 'object' &&
        error !== null &&
        'message' in error &&
        typeof (error as { message: unknown }).message === 'string'
          ? (error as { message: string }).message
          : null
      const message = detail && detail.length <= MAX_TOAST_CHARS ? detail : fallback
      void showRemoteToast(target.window, message)
    }

    const windowName = (href: string) => {
      const text = target.linkText?.trim()
      if (text) return text
      try {
        return new URL(href).hostname || 'New Space'
      } catch {
        return 'New Space'
      }
    }
    const openInWindow = (href: string | (() => Promise<string>), isPrivate: boolean) =>
      afterOnce(async () => {
        const url = typeof href === 'string' ? href : await href()
        return commands
          .openUrlInNewWindow(
            url,
            windowName(url),
            nextAccent(workspacesRef.current.map((w) => w.accent)),
            isPrivate,
          )
          .catch(reportFailure('Opening a new window failed'))
      })

    const pageTab = state?.tabs.find((tab) => tab.id === target.tabId)

    const link = target.linkHref && isWebUrl(target.linkHref) ? target.linkHref : null
    if (link) {
      const followed = () => commands.cleanFollowedLink(link)
      rows.push(
        {
          id: 'link-tab',
          label: 'Open link in new tab',
          run: afterHere(async () => commands.createRelatedTab(target.workspaceId, await followed())),
        },
        {
          id: 'link-bg',
          label: 'Open link in background tab',
          run: afterHere(async () => commands.createBackgroundTab(target.workspaceId, await followed())),
        },
        {
          id: 'link-split',
          label: 'Open link in split view',
          run: afterHere(async () =>
            commands.splitPane(target.workspaceId, 'horizontal', false, await followed()),
          ),
        },
        {
          id: 'link-peek',
          label: 'Preview link',
          run: after(() =>
            emit(PEEK_OPEN_EVENT, {
              href: link,
              text: target.linkText ?? '',
              x: target.peekX,
              y: target.peekY,
            }),
          ),
        },
        ...(target.ephemeral
          ? []
          : [
              {
                id: 'link-window',
                label: 'Open link in new window',
                run: openInWindow(followed, false),
              },
            ]),
        {
          id: 'link-private',
          label: 'Open link in new private window',
          run: openInWindow(followed, true),
        },
        {
          id: 'link-copy',
          label: 'Copy link address',
          run: after(() => commands.copyText(link)),
        },
        ...(target.cleanLink
          ? [
              {
                id: 'link-copy-clean',
                label: `Copy clean link (${target.cleanRemoved.length} removed)`,
                run: after(() => commands.copyText(target.cleanLink as string)),
              },
            ]
          : []),
        ...(target.ephemeral
          ? []
          : [
              {
                id: 'link-bookmark',
                label: 'Bookmark link',
                run: after(() =>
                  commands
                    .addBookmark({
                      workspaceId: target.workspaceId,
                      folderId: null,
                      url: link,
                      title: target.linkText?.trim() || link,
                      faviconHash: null,
                      pinned: false,
                    })
                    .catch(reportFailure('Bookmarking the link failed')),
                ),
              },
            ]),
      )
    }

    const image = target.imageSrc && isWebUrl(target.imageSrc) ? target.imageSrc : null
    if (image) {
      rows.push(
        {
          id: 'image-tab',
          label: 'Open image in new tab',
          run: afterHere(() => commands.createRelatedTab(target.workspaceId, image)),
        },
        {
          id: 'image-save',
          label: 'Save image as…',
          run: after(() =>
            commands
              .saveImageAs(target.workspaceId, image)
              .catch(reportFailure('Saving the image failed')),
          ),
        },
        {
          id: 'image-copy-image',
          label: 'Copy image',
          run: after(() =>
            commands
              .copyImage(target.workspaceId, image)
              .then(() => showRemoteToast(target.window, 'Image copied'))
              .catch(reportFailure('Copying the image failed')),
          ),
        },
        {
          id: 'image-copy',
          label: 'Copy image address',
          run: after(() => commands.copyText(image)),
        },
      )
    }

    // Saving media is not offered; TODO: implement fetch URL entry point.
    const media =
      target.mediaSrc && isWebUrl(target.mediaSrc) && target.mediaKind ? target.mediaSrc : null
    if (media) {
      const noun = target.mediaKind === 'audio' ? 'audio' : 'video'
      rows.push(
        {
          id: 'media-tab',
          label: `Open ${noun} in new tab`,
          run: afterHere(() => commands.createRelatedTab(target.workspaceId, media)),
        },
        {
          id: 'media-copy',
          label: `Copy ${noun} address`,
          run: after(() => commands.copyText(media)),
        },
      )
    }

    const selection = target.selection
    if (selection) {
      const engine = activeEngine(settings)
      rows.push(
        {
          id: 'selection-search',
          label: `Search ${engine.label} for “${ellipsis(selection)}”`,
          run: afterHere(() =>
            commands.createRelatedTab(
              target.workspaceId,
              engine.search(selection.slice(0, MAX_SEARCH_CHARS)),
            ),
          ),
        },
        {
          id: 'selection-copy',
          label: 'Copy',
          run: after(() => commands.copyText(selection)),
        },
        ...(aiReady
          ? [
              {
                id: 'selection-ask',
                label: 'Ask the assistant',
                run: after(() =>
                  emit(AI_ASK_EVENT, {
                    window: target.window,
                    seq: target.seq,
                    question: selection,
                  }),
                ),
              },
            ]
          : []),
      )
    }

    if (target.kind === 'chrome') return rows

    rows.push(
      {
        id: 'page-back',
        label: 'Back',
        disabled: !target.canGoBack,
        run: after(() => commands.tabGoBack(target.tabId)),
      },
      {
        id: 'page-forward',
        label: 'Forward',
        disabled: !target.canGoForward,
        run: after(() => commands.tabGoForward(target.tabId)),
      },
      {
        id: 'page-reload',
        label: 'Reload',
        run: after(() => commands.tabReload(target.tabId)),
      },
      {
        id: 'page-print',
        label: 'Print…',
        run: after(() => commands.printActiveTab(target.tabId)),
      },
      ...(target.reader
        ? []
        : [
            {
              id: 'page-source',
              label: 'View source',
              run: afterOnce(async () => {
                let captured = true
                await commands.capturePageSource(target.tabId).catch((error: unknown) => {
                  captured = false
                  reportFailure('Reading the page source failed')(error)
                })
                if (!captured) return
                await commands.createRelatedTab(
                  target.workspaceId,
                  sourceUrlFor(pageTab?.url ?? '', target.tabId),
                )
              }),
            },
          ]),
      {
        id: 'page-save',
        label: 'Save page…',
        run: after(() =>
          commands
            .savePageArchive(target.tabId, pageTab?.title ?? '', pageTab?.url ?? '')
            .catch(reportFailure('Saving the page failed')),
        ),
      },
      // Private tabs are built without devtools, so this is a no-op.
      ...(target.ephemeral
        ? []
        : [
            {
              id: 'page-inspect',
              label: 'Inspect',
              run: after(() => commands.inspectTab(target.tabId)),
            },
          ]),
    )

    return rows
  }, [target, state, settings, aiReady, close, busyRef, workspacesRef])
}

/** Selected text in a row label, short enough to stay on one line. */
function ellipsis(text: string): string {
  const flat = text.replace(/\s+/g, ' ').trim()
  return flat.length > 24 ? `${flat.slice(0, 24)}…` : flat
}
