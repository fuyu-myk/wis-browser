import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
} from 'react'
import { createPortal } from 'react-dom'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { AccentColor, BrowserState, Workspace, WorkspaceId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { PlusIcon, XIcon } from '@/lib/icons'
import { WisMark } from '@/components/WisMark'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'
import { usePendingRename } from '@/hooks/usePendingRename'

interface WorkspacePillProps {
  state: BrowserState
  /** Notified whenever the popover open state flips. */
  onOpenChange?: (open: boolean) => void
}

const ACCENTS: AccentColor[] = [
  'primary',
  'mint',
  'rose',
  'sun',
  'lila',
  'gray',
  'ink',
  'azur',
]

const POP_MIN_WIDTH = 210

export function WorkspacePill({ state, onOpenChange }: WorkspacePillProps) {
  const [open, setOpen] = useState(false)
  const [popEl, setPopEl] = useState<HTMLDivElement | null>(null)
  const anchorRef = useRef<HTMLButtonElement | null>(null)
  const [style, setStyle] = useState<CSSProperties>({})
  const [placed, setPlaced] = useState(false)
  const { pendingRenameWorkspaceId, clearRename } = usePendingRename()

  const active = state.workspaces.find((w) => w.id === state.active_workspace_id)

  useEffect(() => {
    onOpenChange?.(open)
  }, [open, onOpenChange])

  // Cmd+N (and any other "rename this workspace" trigger) sets a pending
  // rename in context. Open the popover so the rename input is visible.
  useEffect(() => {
    if (pendingRenameWorkspaceId && !open) setOpen(true)
  }, [pendingRenameWorkspaceId, open])

  // Drop the pending rename when the popover closes by any path.
  useEffect(() => {
    if (!open && pendingRenameWorkspaceId) clearRename()
  }, [open, pendingRenameWorkspaceId, clearRename])

  useLayoutEffect(() => {
    if (!open) {
      setPlaced(false)
      return
    }

    const initialSidebar = anchorRef.current?.closest('.kb-side') as HTMLElement | null
    const startsCollapsed = !!initialSidebar && !initialSidebar.classList.contains('is-expanded')
    const place = () => {
      const anchor = anchorRef.current
      if (!anchor) return

      const rect = anchor.getBoundingClientRect()
      const sidebar = anchor.closest('.kb-side') as HTMLElement | null
      const bounds = sidebar?.getBoundingClientRect()
      const margin = 8
      const innerLeft = (bounds?.left ?? 0) + margin
      const innerRight = (bounds?.right ?? window.innerWidth) - margin
      const maxWidth = Math.max(POP_MIN_WIDTH, innerRight - innerLeft)
      const width = Math.min(POP_MIN_WIDTH, maxWidth)
      const left = Math.max(innerLeft, Math.min(rect.left, innerRight - width))

      setStyle({
        top: rect.bottom + 6,
        left,
        width,
      })
    }
    place()

    if (!startsCollapsed) setPlaced(true)
    window.addEventListener('resize', place)
    window.addEventListener('scroll', place, true)

    const anchor = anchorRef.current
    const sidebar = anchor?.closest('.kb-side') as HTMLElement | null
    const observer = anchor ? new ResizeObserver(place) : null
    if (anchor && observer) observer.observe(anchor)
    const onTransitionEnd = (e: TransitionEvent) => {
      place()
      if (e.propertyName === 'width') setPlaced(true)
    }
    if (sidebar) {
      sidebar.addEventListener('transitionrun', place)
      sidebar.addEventListener('transitionend', onTransitionEnd)
    }
    return () => {
      window.removeEventListener('resize', place)
      window.removeEventListener('scroll', place, true)
      observer?.disconnect()
      if (sidebar) {
        sidebar.removeEventListener('transitionrun', place)
        sidebar.removeEventListener('transitionend', onTransitionEnd)
      }
    }
  }, [open])

  useEffect(() => {
    if (!open) return
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node | null
      if (!t) return
      if (popEl?.contains(t)) return
      if (anchorRef.current?.contains(t)) return
      setOpen(false)
    }
    document.addEventListener('mousedown', onDown)
    return () => document.removeEventListener('mousedown', onDown)
  }, [open, popEl])

  // Always-active dismiss listeners.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false)
    }
    const onCloseRequest = () => setOpen(false)
    document.addEventListener('keydown', onKey)
    window.addEventListener('wis-close-workspace-popover', onCloseRequest)
    return () => {
      document.removeEventListener('keydown', onKey)
      window.removeEventListener('wis-close-workspace-popover', onCloseRequest)
    }
  }, [])

  useEffect(() => {
    if (!open) return
    void commands.focusWebview('main').catch(() => undefined)
  }, [open])

  return (
    <div className="kb-ws-pill-wrap">
      <button
        ref={anchorRef}
        type="button"
        className={cx('kb-ws-pill', { 'is-private': active?.ephemeral })}
        aria-haspopup="dialog"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        {active?.ephemeral ? (
          <span className="kb-ws-private-badge" aria-hidden="true">
            <WisMark variant="solid-closed" size={16} />
          </span>
        ) : (
          <span className={cx('kb-ws-dot', `accent-${active?.accent ?? 'primary'}`)} />
        )}
        <span className="kb-ws-name">{active?.name ?? 'Workspace'}</span>
      </button>
      {open &&
        createPortal(
          <WorkspacePopover
            popRef={setPopEl}
            style={{ ...style, visibility: placed ? 'visible' : 'hidden' }}
            workspaces={state.workspaces}
            activeId={state.active_workspace_id}
            onPick={() => setOpen(false)}
            initialEditingId={pendingRenameWorkspaceId}
            onEditingFinished={clearRename}
          />,
          document.body,
        )}
    </div>
  )
}

interface WorkspacePopoverProps {
  workspaces: Workspace[]
  activeId: WorkspaceId
  onPick: () => void
  popRef: (el: HTMLDivElement | null) => void
  style: CSSProperties
  /** Workspace id to mount in rename mode, e.g. just-created via Cmd+N. */
  initialEditingId?: WorkspaceId | null
  /** Called when rename mode exits (commit, cancel, or row change). */
  onEditingFinished?: () => void
}

function WorkspacePopover({
  workspaces,
  activeId,
  onPick,
  popRef,
  style,
  initialEditingId,
  onEditingFinished,
}: WorkspacePopoverProps) {
  const qc = useQueryClient()
  const [editingId, setEditingId] = useState<WorkspaceId | null>(initialEditingId ?? null)
  const [recoloringId, setRecoloringId] = useState<WorkspaceId | null>(null)
  const [confirmingDeleteId, setConfirmingDeleteId] = useState<WorkspaceId | null>(null)

  const create = useMutation({
    mutationFn: () => commands.createWorkspace('New workspace', 'primary'),
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })

  const createPrivate = useMutation({
    mutationFn: async () => {
      const ws = await commands.createPrivateWorkspace('Private', 'ink')
      await commands.setActiveWorkspace(ws.id)
      return ws
    },
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY })
      onPick()
    },
  })

  const switchTo = useMutation({
    mutationFn: (id: WorkspaceId) => commands.setActiveWorkspace(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })

  const rename = useMutation({
    mutationFn: ({ id, name }: { id: WorkspaceId; name: string }) =>
      commands.renameWorkspace(id, name),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY })
      setEditingId(null)
      onEditingFinished?.()
    },
  })

  const recolor = useMutation({
    mutationFn: ({ id, accent }: { id: WorkspaceId; accent: AccentColor }) =>
      commands.recolorWorkspace(id, accent),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY })
      setRecoloringId(null)
    },
  })

  const remove = useMutation({
    mutationFn: (id: WorkspaceId) => commands.deleteWorkspace(id),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY })
      setConfirmingDeleteId(null)
    },
  })

  const confirmingWorkspace = workspaces.find((w) => w.id === confirmingDeleteId)

  return (
    <div
      ref={popRef}
      className="kb-ws-pop"
      role="dialog"
      aria-label="Workspaces"
      style={style}
    >
      <div className="kb-ws-pop-head">
        <span className="kb-ws-pop-title">Workspaces</span>
        <div className="kb-ws-pop-actions">
          <button
            type="button"
            className="kb-ws-pop-add is-private"
            aria-label="New private workspace"
            title="New private workspace"
            onClick={() => createPrivate.mutate()}
            disabled={createPrivate.isPending}
          >
            <WisMark variant="solid-closed" size={15} />
          </button>
          <button
            type="button"
            className="kb-ws-pop-add"
            aria-label="New workspace"
            onClick={() => create.mutate()}
            disabled={create.isPending}
          >
            <PlusIcon />
          </button>
        </div>
      </div>
      <ul className="kb-ws-list">
        {workspaces.map((w) => (
          <WorkspaceRow
            key={w.id}
            ws={w}
            isActive={w.id === activeId}
            isEditing={editingId === w.id}
            isRecoloring={recoloringId === w.id}
            canDelete={workspaces.length > 1}
            onPick={() => {
              switchTo.mutate(w.id)
              onPick()
            }}
            onRenameStart={() => setEditingId(w.id)}
            onRenameSubmit={(name) => rename.mutate({ id: w.id, name })}
            onRecolorToggle={() =>
              setRecoloringId((prev) => (prev === w.id ? null : w.id))
            }
            onRecolor={(accent) => recolor.mutate({ id: w.id, accent })}
            onDeleteRequest={() => setConfirmingDeleteId(w.id)}
          />
        ))}
      </ul>

      {confirmingWorkspace && (
        <div className="kb-ws-confirm" role="alertdialog" aria-label="Confirm delete">
          <p className="kb-ws-confirm-msg">
            Delete <strong>{confirmingWorkspace.name}</strong>? Its tabs will be lost.
          </p>
          <div className="kb-ws-confirm-actions">
            <button type="button" onClick={() => setConfirmingDeleteId(null)}>
              Cancel
            </button>
            <button
              type="button"
              className="danger"
              onClick={() => remove.mutate(confirmingWorkspace.id)}
              disabled={remove.isPending}
            >
              Delete
            </button>
          </div>
        </div>
      )}
    </div>
  )
}

interface WorkspaceRowProps {
  ws: Workspace
  isActive: boolean
  isEditing: boolean
  isRecoloring: boolean
  canDelete: boolean
  onPick: () => void
  onRenameStart: () => void
  onRenameSubmit: (name: string) => void
  onRecolorToggle: () => void
  onRecolor: (accent: AccentColor) => void
  onDeleteRequest: () => void
}

function WorkspaceRow({
  ws,
  isActive,
  isEditing,
  isRecoloring,
  canDelete,
  onPick,
  onRenameStart,
  onRenameSubmit,
  onRecolorToggle,
  onRecolor,
  onDeleteRequest,
}: WorkspaceRowProps) {
  const [draft, setDraft] = useState(ws.name)
  const inputRef = useRef<HTMLInputElement | null>(null)

  useEffect(() => {
    if (isEditing) {
      setDraft(ws.name)
      requestAnimationFrame(() => inputRef.current?.select())
    }
  }, [isEditing, ws.name])

  const submitName = () => {
    const trimmed = draft.trim()
    if (trimmed && trimmed !== ws.name) onRenameSubmit(trimmed)
    else onRenameSubmit(ws.name)
  }

  return (
    <li className={cx('kb-ws-row', { active: isActive, 'is-private': ws.ephemeral })}>
      <div className="kb-ws-row-main">
        {ws.ephemeral ? (
          <span className="kb-ws-row-private" aria-label="Private workspace" title="Private workspace">
            <WisMark variant="solid-closed" size={15} />
          </span>
        ) : (
          <button
            type="button"
            className={cx('kb-ws-dot-btn', `accent-${ws.accent}`, { active: isRecoloring })}
            aria-label={`Recolor ${ws.name}`}
            onClick={(e) => {
              e.stopPropagation()
              onRecolorToggle()
            }}
          />
        )}
        {isEditing ? (
          <input
            ref={inputRef}
            className="kb-ws-row-input"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={submitName}
            onKeyDown={(e) => {
              if (e.key === 'Enter') submitName()
              else if (e.key === 'Escape') onRenameSubmit(ws.name)
            }}
            onClick={(e) => e.stopPropagation()}
          />
        ) : (
          <span
            className="kb-ws-row-name"
            role="button"
            tabIndex={0}
            title="Right-click to rename"
            onClick={onPick}
            onContextMenu={(e) => {
              e.preventDefault()
              e.stopPropagation()
              onRenameStart()
            }}
            onKeyDown={(e) => {
              if (e.key === 'Enter') onPick()
              else if (e.key === 'F2') onRenameStart()
            }}
          >
            {ws.name}
          </span>
        )}
      </div>
      <button
        type="button"
        className="kb-ws-row-delete"
        aria-label={canDelete ? `Delete ${ws.name}` : 'Cannot delete the only workspace'}
        disabled={!canDelete}
        onClick={(e) => {
          e.stopPropagation()
          if (canDelete) onDeleteRequest()
        }}
      >
        <XIcon />
      </button>
      {isRecoloring && (
        <div className="kb-ws-swatches" role="menu" aria-label="Recolor workspace">
          {ACCENTS.map((accent) => (
            <button
              key={accent}
              type="button"
              className={cx('kb-ws-swatch', `accent-${accent}`, {
                on: accent === ws.accent,
              })}
              aria-label={accent}
              onClick={(e) => {
                e.stopPropagation()
                onRecolor(accent)
              }}
            />
          ))}
        </div>
      )}
    </li>
  )
}
