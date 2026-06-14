/**
 * New Window free-workspace picker, hosted by the `overlay-<window>-windowpicker`
 * webview. The chrome (`useWindowPickerOverlay`) sizes this webview to fill the
 * content area so the dimmed backdrop captures click-outside; the picker itself
 * is a centered floating panel.
 *
 * Selecting a workspace opens a new OS window bound to it (`new_window`);
 * "New empty workspace" creates a fresh persistent workspace (with a first tab)
 * and opens a window for it. One workspace per window, so the list (from
 * `free_workspaces`) excludes any workspace already shown by a window.
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import type { KeyboardEvent } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

const SINK_MS = 220

import { commands } from '@/bindings/commands'
import type { Workspace, WorkspaceId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { nextAccent } from '@/lib/accents'
import { NEW_TAB_URL } from '@/lib/url'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'

const PICKER_FOCUS_EVENT = 'wis-overlay-windowpicker-focus'
const PICKER_CLOSED_EVENT = 'wis-overlay-windowpicker-closed'

export function WindowPickerSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)

  const [free, setFree] = useState<Workspace[]>([])
  const [highlight, setHighlight] = useState(0)
  const [closing, setClosing] = useState(false)
  const panelRef = useRef<HTMLDivElement | null>(null)
  const busyRef = useRef(false)

  const rowCount = free.length + 1

  const close = useCallback(async () => {
    setClosing(true)
    await new Promise((resolve) => setTimeout(resolve, SINK_MS))
    setClosing(false)
    await commands.hideOverlay('windowpicker').catch(() => undefined)
    await emit(PICKER_CLOSED_EVENT).catch(() => undefined)
  }, [])

  const openWorkspace = useCallback(
    async (id: WorkspaceId) => {
      if (busyRef.current) return
      busyRef.current = true
      await commands.newWindow(id).catch(() => undefined)
      busyRef.current = false
      await close()
    },
    [close],
  )

  const createNew = useCallback(async () => {
    if (busyRef.current || !state) return
    busyRef.current = true
    try {
      const accent = nextAccent(state.workspaces.map((w) => w.accent))
      const ws = await commands.createWorkspace('New Space', accent)
      await commands.createTab(ws.id, NEW_TAB_URL)
      await commands.newWindow(ws.id)
    } catch {
      /* surfaced by the failed action; just close */
    }
    busyRef.current = false
    await close()
  }, [state, close])

  const activate = useCallback(
    (i: number) => {
      if (i < free.length) void openWorkspace(free[i].id)
      else void createNew()
    },
    [free, openWorkspace, createNew],
  )

  // Refresh the free list and focus whenever the chrome opens the picker.
  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<void>(PICKER_FOCUS_EVENT, () => {
      setClosing(false)
      setHighlight(0)
      void commands
        .freeWorkspaces()
        .then((ws) => {
          if (!cancelled) setFree(ws)
        })
        .catch(() => setFree([]))
      void commands.focusSelf().catch(() => undefined)
      requestAnimationFrame(() => panelRef.current?.focus())
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === 'Escape') {
      e.preventDefault()
      void close()
    } else if (e.key === 'ArrowDown') {
      e.preventDefault()
      setHighlight((h) => Math.min(rowCount - 1, h + 1))
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setHighlight((h) => Math.max(0, h - 1))
    } else if (e.key === 'Enter') {
      e.preventDefault()
      activate(highlight)
    }
  }

  const glassOn = state?.settings.glass_enabled ?? true

  return (
    <div
      className={cx('kb-palette-backdrop', { 'glass-on': glassOn, 'is-closing': closing })}
      role="presentation"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) void close()
      }}
    >
      <div
        ref={panelRef}
        tabIndex={-1}
        className={cx('kb-palette', 'kb-winpicker', { 'glass-on': glassOn, 'is-closing': closing })}
        role="dialog"
        aria-label="Open a workspace in a new window"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={onKeyDown}
      >
        <div className="kb-winpicker-title">Open in a new window</div>
        <div className="kb-palette-results">
          {free.map((w, i) => (
            <button
              key={w.id}
              type="button"
              className={cx('kb-palette-row', 'kb-winpicker-row', { highlighted: i === highlight })}
              onMouseEnter={() => setHighlight(i)}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => void openWorkspace(w.id)}
            >
              <span className={cx('kb-winpicker-dot', `accent-${w.accent}`)} aria-hidden />
              <span className="kb-palette-row-title">{w.name}</span>
            </button>
          ))}
          <button
            type="button"
            className={cx('kb-palette-row', 'kb-winpicker-row', 'kb-winpicker-new', {
              highlighted: highlight === free.length,
            })}
            onMouseEnter={() => setHighlight(free.length)}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => void createNew()}
          >
            <span className="kb-winpicker-dot-new" aria-hidden>
              +
            </span>
            <span className="kb-palette-row-title">New empty workspace</span>
          </button>
        </div>
      </div>
    </div>
  )
}
