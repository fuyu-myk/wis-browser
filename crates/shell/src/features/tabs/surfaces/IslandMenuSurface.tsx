/**
 * Options popover for a top-strip tab island, hosted by `overlay-islandmenu`.
 * Recolor (accent swatches), rename (inline input), and delete the folder.
 */

import { useEffect, useRef, useState } from 'react'
import { emit, listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { AccentColor } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { IconPicker } from '@/features/workspaces/components/IconPicker'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'
import { menusGlass } from '@/lib/glass'
import {
  ISLAND_MENU_DONE,
  ISLAND_MENU_OPEN,
  ISLAND_MENU_READY,
  type IslandMenuTarget,
} from '@/features/tabs/hooks/useIslandMenu'
import { moveFocusByArrow } from '@/lib/arrowNav'

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

export function IslandMenuSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)

  const [target, setTarget] = useState<IslandMenuTarget | null>(null)
  const [draft, setDraft] = useState('')
  const [confirming, setConfirming] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<IslandMenuTarget>(ISLAND_MENU_OPEN, (e) => {
      setTarget(e.payload)
      setDraft(e.payload.name)
      setConfirming(false)
      void commands.focusSelf().catch(() => undefined)
      requestAnimationFrame(() => inputRef.current?.select())
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    void emit(ISLAND_MENU_READY)
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  const close = () => {
    setTarget(null)
    void commands.hideOverlay('islandmenu').catch(() => undefined)
    void emit(ISLAND_MENU_DONE)
  }

  if (!target) return null

  const recolor = (accent: AccentColor) => {
    void commands.recolorTabFolder(target.workspaceId, target.folderId, accent).catch(() => undefined)
    close()
  }
  const submitName = () => {
    const trimmed = draft.trim()
    if (trimmed && trimmed !== target.name) {
      void commands.renameTabFolder(target.workspaceId, target.folderId, trimmed).catch(() => undefined)
    }
    close()
  }
  const remove = () => {
    void commands.deleteTabFolder(target.workspaceId, target.folderId).catch(() => undefined)
    close()
  }

  const glassMenus = menusGlass(state?.settings)
  const folderIcon =
    state?.workspaces
      .find((w) => w.id === target.workspaceId)
      ?.tab_folders.find((f) => f.id === target.folderId)?.icon ?? null

  const setIcon = (icon: string | null) => {
    void commands
      .setTabFolderIcon(target.workspaceId, target.folderId, icon)
      .catch(() => undefined)
  }

  return (
    <div className={cx('kb-islandmenu', { 'glass-menus': glassMenus })} role="menu" onKeyDown={(e) => moveFocusByArrow(e, e.currentTarget, 'button', 'both')}>
      <IconPicker value={folderIcon} onPick={setIcon} />
      <div className="kb-islandmenu-swatches" role="group" aria-label="Recolor folder">
        {ACCENTS.map((accent) => (
          <button
            key={accent}
            type="button"
            className={cx('kb-ws-swatch', `accent-${accent}`, { on: accent === target.accent })}
            aria-label={accent}
            onClick={() => recolor(accent)}
          />
        ))}
      </div>
      <input
        ref={inputRef}
        className="kb-islandmenu-input"
        value={draft}
        placeholder="Folder name"
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') submitName()
          else if (e.key === 'Escape') close()
        }}
      />
      <button
        type="button"
        className="kb-islandmenu-item danger"
        onClick={() => (confirming ? remove() : setConfirming(true))}
      >
        {confirming ? 'Click again to delete' : 'Delete folder'}
      </button>
    </div>
  )
}
