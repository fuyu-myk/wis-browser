import { useEffect, useRef, type ReactNode } from 'react'

import { cx } from '@/lib/cx'

interface ConfirmDialogProps {
  open: boolean
  title: string
  /** Optional supporting copy under the title. */
  message?: ReactNode
  confirmLabel: string
  cancelLabel?: string
  danger?: boolean
  onConfirm: () => void
  onCancel: () => void
}

/**
 * A small modal confirmation dialog rendered in the chrome layer. Used by
 * chrome pages (history, "clear browsing data") where no native webview
 * occludes the chrome HTML, so a plain fixed overlay suffices. Esc or a
 * backdrop click cancels; the confirm button is focused on open.
 */
export function ConfirmDialog({
  open,
  title,
  message,
  confirmLabel,
  cancelLabel = 'Cancel',
  danger = false,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const confirmRef = useRef<HTMLButtonElement | null>(null)

  useEffect(() => {
    if (!open) return
    confirmRef.current?.focus()
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onCancel()
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [open, onCancel])

  if (!open) return null

  return (
    <div
      className="kb-modal-backdrop"
      role="presentation"
      onPointerDown={(e) => {
        if (e.target === e.currentTarget) onCancel()
      }}
    >
      <div className="kb-modal" role="dialog" aria-modal="true" aria-label={title}>
        <h2 className="kb-modal-title">{title}</h2>
        {message ? <p className="kb-modal-msg">{message}</p> : null}
        <div className="kb-modal-actions">
          <button type="button" className="kb-modal-btn" onClick={onCancel}>
            {cancelLabel}
          </button>
          <button
            ref={confirmRef}
            type="button"
            className={cx('kb-modal-btn', 'is-primary', { 'is-danger': danger })}
            onClick={onConfirm}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  )
}
