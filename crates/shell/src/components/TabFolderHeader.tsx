import {
  useEffect,
  useRef,
  useState,
  type MutableRefObject,
  type PointerEvent as ReactPointerEvent,
} from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { AccentColor, TabFolder, WorkspaceId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { MoreIcon } from '@/lib/icons'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'
import { renderIconRef } from '@/lib/iconGlyphs'
import { IconPicker } from '@/components/IconPicker'

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

const SIDEBAR_COLLAPSED_EVENT = 'wis-sidebar-collapsed'

interface TabFolderHeaderProps {
  workspaceId: WorkspaceId
  folder: TabFolder
  /** Number of tabs in the folder (shown when collapsed). */
  count: number
  /** Highlight while a tab is dragged over this folder. */
  isDropTarget: boolean
  /** True when this folder is the target for new tabs. */
  selected: boolean
  /** True when the active tab lives in this folder (drives the folder-level
   * active indicator when the tab's own row isn't visible). */
  containsActive: boolean
  /** Begin a folder reorder drag from the header. */
  onHeaderPointerDown?: (e: ReactPointerEvent) => void
  /** True after a drag, so the ensuing click doesn't also select the folder. */
  dragRef?: MutableRefObject<boolean>
  /** Reports while the menu or rename is open (keeps the sidebar from collapsing). */
  onEditingChange?: (editing: boolean) => void
}

/**
 * Folder header: a colored name pill (only the name is tinted, sized to its
 * text). Clicking the row selects the folder as the target for new tabs
 * (highlighted in the theme color); the chevron toggles collapse; right-click
 * (or the `…` button) opens an options popover for rename, recolor, and delete.
 * Collapsing the sidebar discards an in-progress edit and closes the popover.
 */
export function TabFolderHeader({
  workspaceId,
  folder,
  count,
  isDropTarget,
  selected,
  containsActive,
  onHeaderPointerDown,
  dragRef,
  onEditingChange,
}: TabFolderHeaderProps) {
  const qc = useQueryClient()
  const invalidate = () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY })

  const select = useMutation({
    mutationFn: () => commands.selectTabFolder(workspaceId, selected ? null : folder.id),
    onSuccess: invalidate,
  })
  const collapse = useMutation({
    mutationFn: (collapsed: boolean) =>
      commands.setTabFolderCollapsed(workspaceId, folder.id, collapsed),
    onSuccess: invalidate,
  })
  const rename = useMutation({
    mutationFn: (name: string) => commands.renameTabFolder(workspaceId, folder.id, name),
    onSuccess: invalidate,
  })
  const setIcon = useMutation({
    mutationFn: (icon: string | null) =>
      commands.setTabFolderIcon(workspaceId, folder.id, icon),
    onSuccess: () => void qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })
  const recolor = useMutation({
    mutationFn: (accent: AccentColor) => commands.recolorTabFolder(workspaceId, folder.id, accent),
    onSuccess: invalidate,
  })
  const remove = useMutation({
    mutationFn: () => commands.deleteTabFolder(workspaceId, folder.id),
    onSuccess: invalidate,
  })

  const [menuOpen, setMenuOpen] = useState(false)
  const [renaming, setRenaming] = useState(false)
  const [draft, setDraft] = useState(folder.name)
  const [confirming, setConfirming] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)
  // Set true right before a discarding unmount so the input's blur skips saving.
  const cancelRenameRef = useRef(false)

  const editing = menuOpen || renaming
  const onEditingChangeRef = useRef(onEditingChange)
  onEditingChangeRef.current = onEditingChange
  useEffect(() => {
    onEditingChangeRef.current?.(editing)
  }, [editing])
  useEffect(() => () => onEditingChangeRef.current?.(false), [])

  // Collapsing the sidebar discards any in-progress edit and closes the popover.
  useEffect(() => {
    const onCollapsed = () => {
      cancelRenameRef.current = true
      setRenaming(false)
      setMenuOpen(false)
      setConfirming(false)
    }
    window.addEventListener(SIDEBAR_COLLAPSED_EVENT, onCollapsed)
    return () => window.removeEventListener(SIDEBAR_COLLAPSED_EVENT, onCollapsed)
  }, [])

  useEffect(() => {
    if (renaming) {
      setDraft(folder.name)
      requestAnimationFrame(() => inputRef.current?.select())
    }
  }, [renaming, folder.name])

  const submitName = () => {
    if (cancelRenameRef.current) {
      cancelRenameRef.current = false
      setRenaming(false)
      return
    }
    const trimmed = draft.trim()
    if (trimmed && trimmed !== folder.name) rename.mutate(trimmed)
    setRenaming(false)
  }

  const startRename = () => {
    cancelRenameRef.current = false
    setMenuOpen(false)
    setRenaming(true)
  }

  return (
    <div
      className={cx('kb-tabfolder-head-wrap', {
        'has-active': containsActive,
        'folder-collapsed': folder.collapsed,
      })}
    >
      <div
        className={cx('kb-tabfolder-head', {
          collapsed: folder.collapsed,
          selected,
          'is-drop': isDropTarget,
        })}
        data-folder-id={folder.id}
        role="button"
        tabIndex={0}
        aria-pressed={selected}
        onPointerDown={(e) => onHeaderPointerDown?.(e)}
        onClick={() => {
          if (renaming) return
          if (dragRef?.current) {
            dragRef.current = false
            return
          }
          select.mutate()
        }}
        onContextMenu={(e) => {
          e.preventDefault()
          setConfirming(false)
          setMenuOpen((v) => !v)
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            // IDE-style: Enter on the selected folder starts renaming;
            // otherwise it selects it first.
            e.preventDefault()
            if (selected) startRename()
            else select.mutate()
          } else if (e.key === ' ') {
            e.preventDefault()
            select.mutate()
          }
        }}
      >
        <span className="kb-tabfolder-dot" aria-hidden>
          {renderIconRef(folder.icon, 10)}
        </span>
        <button
          type="button"
          className="kb-tabfolder-chevron-btn"
          aria-label={folder.collapsed ? 'Expand folder' : 'Collapse folder'}
          onClick={(e) => {
            e.stopPropagation()
            collapse.mutate(!folder.collapsed)
          }}
        >
          <ChevronIcon collapsed={folder.collapsed} />
        </button>
        {renaming ? (
          <input
            ref={inputRef}
            className="kb-tabfolder-rename"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onClick={(e) => e.stopPropagation()}
            onBlur={submitName}
            onKeyDown={(e) => {
              e.stopPropagation()
              if (e.key === 'Enter') submitName()
              else if (e.key === 'Escape') {
                cancelRenameRef.current = true
                setRenaming(false)
              }
            }}
          />
        ) : (
          <span
            className="kb-tabfolder-name"
            onDoubleClick={(e) => {
              e.stopPropagation()
              startRename()
            }}
          >
            {renderIconRef(folder.icon, 11)}
            {folder.name}
          </span>
        )}
        <span className="kb-tabfolder-count">{count}</span>
        <button
          type="button"
          className="kb-tabfolder-more"
          aria-label="Folder options"
          onClick={(e) => {
            e.stopPropagation()
            setMenuOpen((v) => !v)
            setConfirming(false)
          }}
        >
          <MoreIcon />
        </button>
      </div>

      {menuOpen && (
        <div className="kb-tabfolder-menu" role="menu">
          <IconPicker
            value={folder.icon}
            onPick={(icon) => setIcon.mutate(icon)}
          />
          <div className="kb-tabfolder-swatches" role="group" aria-label="Recolor folder">
            {ACCENTS.map((accent) => (
              <button
                key={accent}
                type="button"
                className={cx('kb-ws-swatch', `accent-${accent}`, {
                  on: accent === folder.accent,
                })}
                aria-label={accent}
                onClick={() => {
                  recolor.mutate(accent)
                  setMenuOpen(false)
                }}
              />
            ))}
          </div>
          <button type="button" className="kb-tabfolder-menu-item" onClick={startRename}>
            Rename
          </button>
          {confirming ? (
            <div className="kb-tabfolder-confirm">
              <span>Delete folder? Tabs stay open.</span>
              <div className="kb-tabfolder-confirm-actions">
                <button type="button" onClick={() => setConfirming(false)}>
                  Cancel
                </button>
                <button
                  type="button"
                  className="danger"
                  onClick={() => {
                    remove.mutate()
                    setMenuOpen(false)
                  }}
                >
                  Delete
                </button>
              </div>
            </div>
          ) : (
            <button
              type="button"
              className="kb-tabfolder-menu-item danger"
              onClick={() => setConfirming(true)}
            >
              Delete folder
            </button>
          )}
        </div>
      )}
    </div>
  )
}

function ChevronIcon({ collapsed }: { collapsed: boolean }) {
  return (
    <svg
      className={cx('kb-tabfolder-chevron', { collapsed })}
      width="12"
      height="12"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="m6 9 6 6 6-6" />
    </svg>
  )
}
