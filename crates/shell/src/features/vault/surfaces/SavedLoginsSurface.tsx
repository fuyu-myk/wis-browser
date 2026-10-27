/**
 * The saved-logins card, hosted by the `overlay-logins` webview.
 *
 * Picking a login unlocks a locked vault first, so Touch ID comes only after
 * the choice. Clicks just after the card appears are ignored, so a page cannot
 * focus a field under a click already on its way and have it pick a login.
 */

import { useEffect, useRef, useState } from 'react'
import { emit, listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { commands } from '@/bindings/commands'
import { CARD_PLACED_EVENT, useWheelStepsAside, type CardPlaced } from '@/features/vault/hooks/useAnchoredCard'
import { useBrowserState } from '@/hooks/useBrowserState'
import {
  LOGINS_CLOSE_EVENT,
  LOGINS_READY_EVENT,
  LOGINS_SHOW_EVENT,
  type SavedLogins,
} from '@/features/vault/hooks/useSavedLogins'
import { useThemeApply } from '@/hooks/useThemeApply'
import { cx } from '@/lib/cx'
import { menusGlass } from '@/lib/glass'
import { GlobeIcon } from '@/lib/icons'
import { errorMessage } from '@/lib/errors'
import { showRemoteToast } from '@/lib/toast'
import { VAULTMENU_FILL_EVENT, type VaultMenuFill } from '@/features/vault/lib/vaultMenu'
import { unlockProblem } from '@/features/vault/lib/vaultRecovery'
import { usePickerKeys } from '@/features/vault/hooks/usePickerKeys'
import { FIELD_PICK_EVENT } from '@/features/vault/hooks/useAnchoredCard'
import { moveFocusByArrow } from '@/lib/arrowNav'

const EARLY_CLICK_MS = 500

export function SavedLoginsSurface() {
  const { data: state } = useBrowserState()
  useThemeApply(state?.settings)
  useWheelStepsAside('logins')

  const [pick, setPick] = useState<SavedLogins | null>(null)
  const [busy, setBusy] = useState(false)
  const shownAt = useRef(0)

  useEffect(() => {
    const label = getCurrentWindow().label
    const placed = listen<CardPlaced>(CARD_PLACED_EVENT, (event) => {
      if (event.payload.name === 'logins' && event.payload.window === label) {
        shownAt.current = performance.now()
      }
    })
    return () => void placed.then((unlisten) => unlisten())
  }, [])

  useEffect(() => {
    let unlisten: (() => void) | null = null
    let cancelled = false
    const label = getCurrentWindow().label
    void listen<SavedLogins>(LOGINS_SHOW_EVENT, (event) => {
      if (event.payload.window !== label) return
      setPick(event.payload)
      shownAt.current = performance.now()
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    void emit(LOGINS_READY_EVENT, label)
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  const choose = async (credentialId: number) => {
    if (!pick || busy || performance.now() - shownAt.current < EARLY_CLICK_MS) return
    setBusy(true)
    try {
      const status = await commands.vaultStatus().catch(() => null)
      if (status && !status.unlocked && status.password) {
        const label = getCurrentWindow().label
        const request: VaultMenuFill = { window: label, tab: pick.tabId, credentialId }
        void emit(VAULTMENU_FILL_EVENT, request)
        setPick(null)
        void emit(LOGINS_CLOSE_EVENT, label)
        return
      }
      if (status && !status.unlocked && !(await commands.vaultUnlock().catch(toastProblem))) return
      await commands.autofillFill(pick.tabId, credentialId)
      setPick(null)
      void emit(LOGINS_CLOSE_EVENT, getCurrentWindow().label)
      void commands.focusWebview(`tab-${pick.tabId}`).catch(() => undefined)
    } catch (error: unknown) {
      void showRemoteToast(getCurrentWindow().label, errorMessage(error))
    } finally {
      setBusy(false)
    }
  }

  usePickerKeys(FIELD_PICK_EVENT, pick?.tabId ?? null, () => {
    setPick(null)
    void emit(LOGINS_CLOSE_EVENT, getCurrentWindow().label)
  })

  if (!pick) return null

  return (
    <div
      className={cx('kb-logins-card', { 'glass-menus': menusGlass(state?.settings) })}
      role="menu"
      onKeyDown={(e) => moveFocusByArrow(e, e.currentTarget, 'button', 'vertical')}
    >
      <div className="kb-logins-head">Saved passwords</div>
      <div className="kb-logins-list">
        {pick.logins.map((login) => (
          <button
            key={login.id}
            type="button"
            role="menuitem"
            className="kb-vaultmenu-row kb-logins-row"
            disabled={busy}
            onClick={() => void choose(login.id)}
          >
            <span className="kb-vaultmenu-ico" aria-hidden>
              <GlobeIcon />
            </span>
            <span className="kb-vaultmenu-user">{login.username || login.host}</span>
          </button>
        ))}
      </div>
    </div>
  )
}

function toastProblem(error: unknown): false {
  void showRemoteToast(getCurrentWindow().label, unlockProblem(error))
  return false
}
