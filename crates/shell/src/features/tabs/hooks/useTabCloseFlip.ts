import { useCallback, useEffect, useLayoutEffect, useRef, useState, type RefObject } from 'react'

import { commands } from '@/bindings/commands'
import type { TabId } from '@/bindings/types'
import { onTabClosing, TAB_EXIT_MS } from '@/features/tabs/lib/tabClose'
import { closeReturnTarget, noteTabClosed, recentTabs } from '@/features/tabs/lib/tabMru'
import { motionStill } from '@/lib/motion'

/** Marks the close-FLIP glide animations so a rapid re-close can cancel them. */
const FLIP_ANIM_ID = 'wis-tab-flip'

const TAB_SELECTOR = '[data-tab-id], [data-mtab-id]'

interface Slot {
  left: number
  width: number
}

/**
 * The top strip's animated close. Every tab's slot is snapshotted before the
 * close lands, so once the closing tab leaves layout each survivor glides from
 * its old slot to its new one. Returns the tabs still playing their exit.
 */
export function useTabCloseFlip(stripRef: RefObject<HTMLDivElement | null>): {
  closingIds: ReadonlySet<TabId>
  closeHere: (id: TabId) => void
} {
  const [closingIds, setClosingIds] = useState<ReadonlySet<TabId>>(new Set())
  const flipFromRef = useRef<Map<string, Slot>>(new Map())
  const captureFlip = useCallback(() => {
    const strip = stripRef.current
    if (!strip) return
    const map = new Map<string, Slot>()
    strip.querySelectorAll<HTMLElement>(TAB_SELECTOR).forEach((el) => {
      const id = el.dataset.tabId ?? el.dataset.mtabId
      if (!id) return
      const r = el.getBoundingClientRect()
      map.set(id, { left: r.left, width: r.width })
    })
    flipFromRef.current = map
  }, [stripRef])
  // Dedupe a single close.
  const handledRef = useRef<Set<TabId>>(new Set())
  const markClosing = useCallback((id: TabId) => {
    setClosingIds((prev) => (prev.has(id) ? prev : new Set(prev).add(id)))
    window.setTimeout(() => {
      handledRef.current.delete(id)
      setClosingIds((prev) => {
        if (!prev.has(id)) return prev
        const next = new Set(prev)
        next.delete(id)
        return next
      })
    }, TAB_EXIT_MS + 400)
  }, [])
  const beginClose = useCallback(
    (id: TabId) => {
      if (handledRef.current.has(id)) return
      handledRef.current.add(id)
      captureFlip()
      markClosing(id)
    },
    [captureFlip, markClosing],
  )
  const closeHere = useCallback(
    (id: TabId) => {
      beginClose(id)
      const returnTo = closeReturnTarget(id)
      noteTabClosed(id)
      void commands.closeTab(id, returnTo, recentTabs()).catch(() => undefined)
    },
    [beginClose],
  )
  useEffect(() => onTabClosing(beginClose), [beginClose])
  useLayoutEffect(() => {
    const from = flipFromRef.current
    if (from.size === 0) return
    flipFromRef.current = new Map()
    const strip = stripRef.current
    if (!strip) return
    const els = Array.from(strip.querySelectorAll<HTMLElement>(TAB_SELECTOR))
    // Prevent stacking animations.
    for (const el of els) {
      for (const a of el.getAnimations()) if (a.id === FLIP_ANIM_ID) a.cancel()
    }
    const moves: Array<{ el: HTMLElement; dx: number; sx: number }> = []
    for (const el of els) {
      const id = el.dataset.tabId ?? el.dataset.mtabId
      if (id === undefined || closingIds.has(id as TabId)) continue
      const old = from.get(id)
      if (old === undefined) continue
      const now = el.getBoundingClientRect()
      const dx = old.left - now.left
      const sx = now.width > 0.5 ? old.width / now.width : 1
      if (Math.abs(dx) > 0.5 || Math.abs(sx - 1) > 0.005) moves.push({ el, dx, sx })
    }
    if (moves.length === 0) return
    const cs = getComputedStyle(els[0])
    const durRaw = cs.getPropertyValue('--reflow-dur').trim()
    const dur = durRaw.endsWith('ms')
      ? parseFloat(durRaw)
      : durRaw.endsWith('s')
        ? parseFloat(durRaw) * 1000
        : 0
    if (!(dur > 0) || motionStill()) return // reduced or off: the snap is intended
    for (const { el, dx, sx } of moves) {
      const anim = el.animate(
        [
          { transform: `translateX(${dx}px) scaleX(${sx})`, transformOrigin: 'left center' },
          { transform: 'translateX(0px) scaleX(1)', transformOrigin: 'left center' },
        ],
        { duration: dur, easing: 'linear' },
      )
      anim.id = FLIP_ANIM_ID
    }
  }, [closingIds, stripRef])
  return { closingIds, closeHere }
}
