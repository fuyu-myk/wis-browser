/**
 * "Peek" preview surface, hosted by the `overlay-peek` webview. Shows a centered
 * card framing the isolated `peek-page` webview (created by `open_peek`), plus a
 * safety-warning banner and a reader toggle.
 *
 * The live page composites above this overlay, so the page area is left as a
 * measured "hole" (`kb-peek-body`); the card chrome (header, warning banner)
 * lives outside that rect so it is never occluded. The warning is rendered as a
 * banner (not a floating popover) for the same reason.
 */

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { LinkAssessment, WorkspaceId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { XIcon } from '@/lib/icons'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'

const PEEK_SHOW_EVENT = 'wis-peek-show'
const PEEK_CLOSE_EVENT = 'wis-peek-close'
const PEEK_READY_EVENT = 'wis-peek-ready'

interface PeekPayload {
  href: string
  text: string
  /** Click point in window coords (set by the chrome) for the grow origin. */
  originX?: number
  originY?: number
}

const RISK_LABEL: Record<LinkAssessment['risk'], string> = {
  low: 'Heads up',
  medium: 'Be careful',
  high: 'Warning',
}

function hostOf(href: string): string {
  try {
    return new URL(href).host || href
  } catch {
    return href
  }
}

export function PeekSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)
  const glassOn = state?.settings.glass_enabled ?? true
  const activeWorkspaceId: WorkspaceId | null = state?.active_workspace_id ?? null

  const [open, setOpen] = useState(false)
  const [href, setHref] = useState('')
  const [assessment, setAssessment] = useState<LinkAssessment | null>(null)
  const [reader, setReader] = useState(false)
  // Extra friction before opening a high-risk link in a real tab.
  const [confirmOpen, setConfirmOpen] = useState(false)

  const bodyRef = useRef<HTMLDivElement | null>(null)
  const hrefRef = useRef('')
  const textRef = useRef('')
  const openedHrefRef = useRef<string | null>(null)
  const confirmOpenRef = useRef(false)
  const openRef = useRef(false)
  hrefRef.current = href
  confirmOpenRef.current = confirmOpen
  openRef.current = open

  // Push the measured body rect to the native preview; create it the first time
  // a given href is seen, then resize on subsequent layout changes.
  const syncPeek = useCallback(() => {
    if (confirmOpenRef.current) return
    const el = bodyRef.current
    const target = hrefRef.current
    if (!el || !target) return
    const r = el.getBoundingClientRect()
    if (r.width < 1 || r.height < 1) return
    if (openedHrefRef.current !== target) {
      openedHrefRef.current = target
      void commands
        .openPeek({
          url: target,
          anchorText: textRef.current || null,
          x: r.x,
          y: r.y,
          width: r.width,
          height: r.height,
        })
        .then(setAssessment)
        .catch(() => undefined)
    } else {
      void commands.setPeekBounds(r.x, r.y, r.width, r.height).catch(() => undefined)
    }
  }, [])

  const close = useCallback(() => {
    openedHrefRef.current = null
    setOpen(false)
    setHref('')
    setAssessment(null)
    setReader(false)
    setConfirmOpen(false)
    void commands.closePeek().catch(() => undefined)
    void emit(PEEK_CLOSE_EVENT)
    void commands.hideOverlay('peek').catch(() => undefined)
  }, [])

  // Receive the link to preview from the chrome. Subscribe once, then announce
  // readiness so the chrome replays the pending peek (the overlay webview is
  // created lazily and may have missed the first show event).
  useEffect(() => {
    let cancelled = false
    let unlisten: UnlistenFn | null = null
    void listen<PeekPayload>(PEEK_SHOW_EVENT, (e) => {
      const p = e.payload
      if (openRef.current && p.href === hrefRef.current) return // de-dupe replays
      textRef.current = p.text
      originRef.current =
        p.originX != null && p.originY != null ? { x: p.originX, y: p.originY } : null
      openedHrefRef.current = null
      setAssessment(null)
      setReader(false)
      setGrown(false)
      setHref(p.href)
      setOpen(true)
    }).then((fn) => {
      if (cancelled) fn()
      else {
        unlisten = fn
        void emit(PEEK_READY_EVENT)
      }
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  // Grow the card from the Opt+clicked link's position (transform-origin at
  // the click point, approximate in split layouts).
  const [grown, setGrown] = useState(false)
  const cardRef = useRef<HTMLDivElement | null>(null)
  const originRef = useRef<{ x: number; y: number } | null>(null)
  useLayoutEffect(() => {
    if (!open) return
    const card = cardRef.current
    if (!card || document.body.classList.contains('motion-off')) {
      setGrown(true)
      return
    }
    const r = card.getBoundingClientRect()
    const o = originRef.current
    card.style.transformOrigin = o
      ? `${Math.round(o.x - r.left)}px ${Math.round(o.y - r.top)}px`
      : '50% 50%'
    const anim = card.animate(
      [
        { transform: 'scale(0.3)', opacity: 0.4 },
        { transform: 'scale(1)', opacity: 1 },
      ],
      { duration: 260, easing: 'cubic-bezier(0.2, 0.7, 0.3, 1)' },
    )
    anim.addEventListener('finish', () => setGrown(true))
    return () => anim.cancel()
  }, [open, href])

  // Esc dismisses (the confirm dialog first, then the peek itself).
  useEffect(() => {
    if (!open) return
    void commands.focusSelf().catch(() => undefined)
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      if (confirmOpenRef.current) {
        setConfirmOpen(false)
        return
      }
      close()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [open, close])

  // Keep the native preview tiled to the body hole (initial + on any resize of
  // the hole, e.g. when the warning banner appears).
  useEffect(() => {
    if (!open || !grown) return
    const el = bodyRef.current
    if (!el) return
    syncPeek()
    const ro = new ResizeObserver(() => syncPeek())
    ro.observe(el)
    const onResize = () => syncPeek()
    window.addEventListener('resize', onResize)
    return () => {
      ro.disconnect()
      window.removeEventListener('resize', onResize)
    }
  }, [open, grown, syncPeek])

  // Hide the live preview offscreen while the confirmation is up so it can't
  // composite over the dialog; restore it on cancel. Also gated on the grow:
  // this path otherwise creates the native preview mid-entrance.
  useEffect(() => {
    if (!open || !grown) return
    if (confirmOpen) {
      void commands.setPeekBounds(100000, 100000, 320, 240).catch(() => undefined)
    } else {
      syncPeek()
    }
  }, [confirmOpen, open, grown, syncPeek])

  const toggleReader = useCallback(() => {
    const next = !reader
    setReader(next)
    void commands.peekSetReader(next).catch(() => undefined)
  }, [reader])

  const doOpenAsTab = useCallback(() => {
    if (activeWorkspaceId && hrefRef.current) {
      void commands.createTab(activeWorkspaceId, hrefRef.current).catch(() => undefined)
    }
    close()
  }, [activeWorkspaceId, close])

  if (!open) return null

  const warnings = assessment?.warnings ?? []
  const risk = assessment?.risk ?? 'low'
  // High-risk links get a deliberate "are you sure" gate before leaving the
  // protected preview; everything else opens straight away.
  const openAsTab = () => {
    if (risk === 'high') setConfirmOpen(true)
    else doOpenAsTab()
  }

  return (
    <div
      className={cx('kb-peek-backdrop', { 'glass-on': glassOn })}
      onClick={close}
      role="presentation"
    >
      <div
        ref={cardRef}
        className={cx('kb-peek-card', { 'glass-on': glassOn })}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="kb-peek-header">
          <span className="kb-peek-badge">Peek</span>
          <span className="kb-peek-host" title={href}>
            {hostOf(href)}
          </span>
          <button
            type="button"
            className={cx('kb-peek-btn', { 'is-on': reader })}
            onClick={toggleReader}
            aria-pressed={reader}
          >
            Reader
          </button>
          <button type="button" className="kb-peek-btn" onClick={openAsTab}>
            Open as tab
          </button>
          <button
            type="button"
            className="kb-peek-btn kb-peek-icon"
            aria-label="Close preview"
            onClick={close}
          >
            <XIcon />
          </button>
        </div>

        {warnings.length > 0 && (
          <div className={cx('kb-peek-warn', `risk-${risk}`)} role="alert">
            <WarnGlyph />
            <div className="kb-peek-warn-body">
              <strong>{RISK_LABEL[risk]}</strong>
              <ul>
                {warnings.map((w) => (
                  <li key={w.code}>{w.message}</li>
                ))}
              </ul>
            </div>
          </div>
        )}

        <div className="kb-peek-body" ref={bodyRef} />
      </div>

      {confirmOpen && (
        <div
          className={cx('kb-peek-confirm-layer', { 'glass-on': glassOn })}
          onClick={(e) => e.stopPropagation()}
          role="presentation"
        >
          <div
            className={cx('kb-peek-confirm', { 'glass-on': glassOn })}
            role="alertdialog"
            aria-modal="true"
            aria-label="High-risk site warning"
          >
            <div className="kb-peek-confirm-icon">
              <WarnGlyph />
            </div>
            <h2 className="kb-peek-confirm-title">Proceed to a high-risk site?</h2>
            <p className="kb-peek-confirm-text">
              Wis flagged <strong>{hostOf(href)}</strong> as high risk. Opening it in a
              normal tab leaves the protected preview: the site will load with full access
              to cookies and storage, like any other site. Only continue if you trust it.
            </p>
            {warnings.length > 0 && (
              <ul className="kb-peek-confirm-list">
                {warnings.map((w) => (
                  <li key={w.code}>{w.message}</li>
                ))}
              </ul>
            )}
            <div className="kb-peek-confirm-actions">
              <button
                type="button"
                className="kb-peek-btn"
                onClick={() => setConfirmOpen(false)}
              >
                Cancel
              </button>
              <button
                type="button"
                className="kb-peek-confirm-proceed"
                onClick={doOpenAsTab}
              >
                Proceed anyway
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}

function WarnGlyph() {
  return (
    <svg
      className="kb-peek-warn-icon"
      width={18}
      height={18}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0Z" />
      <line x1="12" y1="9" x2="12" y2="13" />
      <line x1="12" y1="17" x2="12.01" y2="17" />
    </svg>
  )
}
