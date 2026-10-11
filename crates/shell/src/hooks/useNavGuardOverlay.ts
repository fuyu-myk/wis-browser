/**
 * Chrome-side orchestration for the navigation safety interstitial. The native
 * guard emits `wis-nav-guard` when a risky navigation is halted; this reveals
 * the full-window `navguard` overlay and forwards the payload to the surface via
 * `wis-navguard-show` (emitted twice to cover the first-open race). It hides the
 * overlay on `wis-navguard-close` and keeps it full-window on resize. The event
 * reaches every window; only the one showing the guarded tab's workspace acts.
 */

import { useEffect, useRef } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import type { LinkAssessment } from '@/bindings/types'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useOverlay } from '@/hooks/useOverlay'

const NAV_GUARD_EVENT = 'wis-nav-guard'
const NAV_GUARD_SHOW_EVENT = 'wis-navguard-show'
const NAV_GUARD_CLOSE_EVENT = 'wis-navguard-close'
const NAV_GUARD_READY_EVENT = 'wis-navguard-ready'

interface NavGuardPayload {
  url: string
  host: string
  assessment: LinkAssessment
  navigated: boolean
  tab_id: string | null
}

export function useNavGuardOverlay(): void {
  const overlay = useOverlay('navguard')
  const openRef = useRef(false)
  const pendingRef = useRef<NavGuardPayload | null>(null)
  const { data: state } = useBrowserState()
  const stateRef = useRef(state)
  stateRef.current = state

  useEffect(() => {
    let cancelled = false
    const unlistens: UnlistenFn[] = []

    const fullRect = () => ({
      x: 0,
      y: 0,
      width: window.innerWidth,
      height: window.innerHeight,
    })

    const showFor = async (p: NavGuardPayload) => {
      pendingRef.current = p
      await overlay.show(fullRect())
      openRef.current = true
      void emit(NAV_GUARD_SHOW_EVENT, p)
    }

    void listen<NavGuardPayload>(NAV_GUARD_EVENT, (e) => {
      const current = stateRef.current
      const tab = e.payload.tab_id ? current?.tabs.find((t) => t.id === e.payload.tab_id) : null
      if (tab && tab.workspace_id !== current?.active_workspace_id) return
      void showFor(e.payload)
    }).then((fn) => {
      if (cancelled) fn()
      else unlistens.push(fn)
    })

    // The surface (re)mounted: replay the pending guard so it isn't missed.
    void listen<void>(NAV_GUARD_READY_EVENT, () => {
      if (pendingRef.current) void emit(NAV_GUARD_SHOW_EVENT, pendingRef.current)
    }).then((fn) => {
      if (cancelled) fn()
      else unlistens.push(fn)
    })

    void listen<void>(NAV_GUARD_CLOSE_EVENT, () => {
      openRef.current = false
      pendingRef.current = null
      void overlay.hide().catch(() => undefined)
    }).then((fn) => {
      if (cancelled) fn()
      else unlistens.push(fn)
    })

    const onResize = () => {
      if (openRef.current) void overlay.show(fullRect())
    }
    window.addEventListener('resize', onResize)

    return () => {
      cancelled = true
      unlistens.forEach((fn) => fn())
      window.removeEventListener('resize', onResize)
    }
  }, [overlay])
}
