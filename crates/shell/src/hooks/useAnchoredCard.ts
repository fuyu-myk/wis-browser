/**
 * An overlay card pinned to a field in a tab's page, as the generated-password
 * offer and the saved-logins card are. `vault/signup.rs` reports the field.
 *
 * The card steps aside while the page scrolls and comes back where the field
 * settles. It closes when the field settles out of its pane, focus moves on,
 * the page goes, or the tab stops being the active one. Focus moving on is
 * given a moment, so a field that wants the same card moves it rather than
 * closing and reopening it. While `suspended`, as when the webviews are
 * hidden for a tab drag, the card is hidden but kept.
 */

import { useCallback, useEffect, useMemo, useRef } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { commands } from '@/bindings/commands'
import type { BrowserState, TabId } from '@/bindings/types'
import { useOverlay } from '@/hooks/useOverlay'
import { fieldInWindow, placeOffer, type Rect, type Size } from '@/lib/signupPlacement'

const FIELD_MOVE_EVENT = 'wis-signup-move'
const FIELD_SCROLL_EVENT = 'wis-signup-scroll'
export const FIELD_LEAVE_EVENT = 'wis-signup-leave'
export const FIELD_GONE_EVENT = 'wis-signup-gone'
const CARD_WHEEL_EVENT = 'wis-card-wheel'
export const CARD_PLACED_EVENT = 'wis-card-placed'

export interface CardPlaced {
  name: string
  window: string
}

/** The events that move and close a card, and how it lets its field go. */
export interface CardChannel {
  move: string
  scroll: string
  leave: string
  release: (tabId: TabId) => Promise<void>
}

export const SIGNUP_CHANNEL: CardChannel = {
  move: FIELD_MOVE_EVENT,
  scroll: FIELD_SCROLL_EVENT,
  leave: FIELD_LEAVE_EVENT,
  release: (tabId) => commands.releaseSignupField(tabId),
}

const GAP = 6
/** A wheel over the card that does not scroll the page puts it back after this. */
const WHEEL_RESTORE_MS = 400
const LEAVE_GRACE_MS = 120

/** A field's box in its page's CSS pixels, and that page's viewport width. */
export interface FieldRect extends Rect {
  viewWidth: number
}

interface TabPayload {
  tabId: string
}

interface MovePayload extends TabPayload {
  rect: FieldRect
}

/** Events reach every window, so a card's own carry its window. */
interface CardWheel {
  name: string
  window: string
}

export interface AnchoredCard {
  /** Where a card of `size` goes for the field, or null once it is out of view. */
  placeAt: (tabId: TabId, field: FieldRect, size: Size) => Rect | null
  /** Show the card at `rect` for `tabId`, or move it there if already open. */
  open: (tabId: TabId, rect: Rect, size: Size) => Promise<void>
  openFor: () => TabId | null
  /** `release` also stops the page reporting its field. */
  close: (release: boolean) => void
}

export function activeTabOf(state: BrowserState): TabId | null {
  return state.workspaces.find((w) => w.id === state.active_workspace_id)?.active_tab_id ?? null
}

function paneOf(tabId: TabId): Rect | null {
  return (
    document.querySelector(`[data-pane-leaf="${CSS.escape(tabId)}"]`)?.getBoundingClientRect() ??
    null
  )
}

export function useAnchoredCard(
  name: string,
  state: BrowserState,
  suspended: boolean,
  onClose: () => void,
  channel: CardChannel = SIGNUP_CHANNEL,
): AnchoredCard {
  const { show, hide } = useOverlay(name)
  const openRef = useRef<TabId | null>(null)
  const sizeRef = useRef<Size | null>(null)
  const placedRef = useRef<Rect | null>(null)
  /** The card's pane when it was placed, to follow a pane that only moved. */
  const paneRef = useRef<Rect | null>(null)
  const suspendedRef = useRef(suspended)
  const restoreRef = useRef<number | undefined>(undefined)
  const leaveRef = useRef<number | undefined>(undefined)
  const onCloseRef = useRef(onClose)
  onCloseRef.current = onClose
  const activeTab = activeTabOf(state)

  const placeAt = useCallback((tabId: TabId, field: FieldRect, size: Size): Rect | null => {
    const pane = paneOf(tabId)
    if (!pane) return null
    return placeOffer(fieldInWindow(field, field.viewWidth, pane), pane, size, GAP)
  }, [])

  const reveal = useCallback(
    async (rect: Rect) => {
      await show(rect)
      const placed: CardPlaced = { name, window: getCurrentWindow().label }
      void emit(CARD_PLACED_EVENT, placed).catch(() => undefined)
    },
    [show, name],
  )

  const place = useCallback(
    async (tabId: TabId, rect: Rect) => {
      placedRef.current = rect
      paneRef.current = paneOf(tabId)
      if (!suspendedRef.current) await reveal(rect)
    },
    [reveal],
  )

  const close = useCallback(
    (release: boolean) => {
      window.clearTimeout(restoreRef.current)
      window.clearTimeout(leaveRef.current)
      const tabId = openRef.current
      if (!tabId) return
      openRef.current = null
      placedRef.current = null
      void hide().catch(() => undefined)
      if (release) void channel.release(tabId).catch(() => undefined)
      onCloseRef.current()
    },
    [hide, channel],
  )

  const open = useCallback(
    async (tabId: TabId, rect: Rect, size: Size) => {
      window.clearTimeout(restoreRef.current)
      window.clearTimeout(leaveRef.current)
      const previous = openRef.current
      if (previous && previous !== tabId) {
        void channel.release(previous).catch(() => undefined)
      }
      openRef.current = tabId
      sizeRef.current = size
      await place(tabId, rect)
    },
    [place, channel],
  )

  useEffect(() => {
    let cancelled = false
    const unlistens: UnlistenFn[] = []
    const track = (pending: Promise<UnlistenFn>) =>
      void pending.then((fn) => (cancelled ? fn() : unlistens.push(fn)))

    track(
      listen<MovePayload>(channel.move, (event) => {
        const tabId = event.payload.tabId as TabId
        const size = sizeRef.current
        if (openRef.current !== tabId || !size) return
        const rect = placeAt(tabId, event.payload.rect, size)
        if (!rect) return close(true)
        window.clearTimeout(restoreRef.current)
        void place(tabId, rect).catch(() => undefined)
      }),
    )

    track(
      listen<TabPayload>(channel.scroll, (event) => {
        if (openRef.current !== event.payload.tabId) return
        window.clearTimeout(restoreRef.current)
        void hide().catch(() => undefined)
      }),
    )

    // Card steps aside to hand the rest of the gesture to the page.
    track(
      listen<CardWheel>(CARD_WHEEL_EVENT, (event) => {
        const { name: card, window: label } = event.payload
        if (card !== name || label !== getCurrentWindow().label || !openRef.current) return
        window.clearTimeout(restoreRef.current)
        void hide().catch(() => undefined)
        restoreRef.current = window.setTimeout(() => {
          const placed = placedRef.current
          if (openRef.current && placed && !suspendedRef.current) {
            void reveal(placed).catch(() => undefined)
          }
        }, WHEEL_RESTORE_MS)
      }),
    )

    track(
      listen<TabPayload>(channel.leave, (event) => {
        if (openRef.current !== event.payload.tabId) return
        window.clearTimeout(leaveRef.current)
        leaveRef.current = window.setTimeout(() => close(false), LEAVE_GRACE_MS)
      }),
    )

    track(
      listen<TabPayload>(FIELD_GONE_EVENT, (event) => {
        if (openRef.current === event.payload.tabId) close(false)
      }),
    )

    return () => {
      cancelled = true
      window.clearTimeout(restoreRef.current)
      window.clearTimeout(leaveRef.current)
      unlistens.forEach((fn) => fn())
    }
  }, [name, placeAt, place, reveal, hide, close, channel])

  useEffect(() => {
    suspendedRef.current = suspended
    const tabId = openRef.current
    if (!tabId) return
    if (suspended) {
      window.clearTimeout(restoreRef.current)
      void hide().catch(() => undefined)
      return
    }
    // A pane that changed size makes its page report a move once it settles.
    const was = paneRef.current
    const now = paneOf(tabId)
    const placed = placedRef.current
    if (!was || !now || !placed) return
    if (Math.abs(now.width - was.width) > 0.5 || Math.abs(now.height - was.height) > 0.5) return
    const rect = { ...placed, x: placed.x + now.x - was.x, y: placed.y + now.y - was.y }
    void place(tabId, rect).catch(() => undefined)
  }, [suspended, hide, place])

  // A card belongs to its tab's page, so it does not outlive the tab being
  // the one on screen.
  useEffect(() => {
    if (openRef.current && openRef.current !== activeTab) close(true)
  }, [activeTab, close])

  return useMemo(
    () => ({ placeAt, open, close, openFor: () => openRef.current }),
    [placeAt, open, close],
  )
}

/**
 * For the card's own surface: the card is its own webview, so a wheel over it
 * would scroll nothing. A list that can scroll keeps its wheel.
 */
export function useWheelStepsAside(name: string): void {
  useEffect(() => {
    const onWheel = (event: WheelEvent) => {
      for (let el = event.target as Element | null; el; el = el.parentElement) {
        const overflow = getComputedStyle(el).overflowY
        const scrolls = overflow === 'auto' || overflow === 'scroll'
        if (scrolls && el.scrollHeight > el.clientHeight) return
      }
      void emit(CARD_WHEEL_EVENT, { name, window: getCurrentWindow().label })
    }
    window.addEventListener('wheel', onWheel, { passive: true })
    return () => window.removeEventListener('wheel', onWheel)
  }, [name])
}
