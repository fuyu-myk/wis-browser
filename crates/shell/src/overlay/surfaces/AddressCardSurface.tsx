/**
 * The saved-addresses card, hosted by the `overlay-addresses` webview.
 *
 * Clicks just after the card appears are ignored, so a page cannot focus a
 * field under a click already on its way and have it pick an address.
 */

import { useEffect, useRef, useState } from 'react'
import { emit, listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { commands } from '@/bindings/commands'
import { CARD_PLACED_EVENT, useWheelStepsAside, type CardPlaced } from '@/hooks/useAnchoredCard'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useThemeApply } from '@/hooks/useThemeApply'
import {
  ADDRESSES_CLOSE_EVENT,
  ADDRESSES_READY_EVENT,
  ADDRESSES_RELOAD_EVENT,
  ADDRESSES_SHOW_EVENT,
  fillsLine,
  type AddressesClose,
  type AddressOffer,
} from '@/lib/addressCard'
import { cx } from '@/lib/cx'
import { errorMessage } from '@/lib/errors'
import { menusGlass } from '@/lib/glass'
import { LockIcon } from '@/lib/icons'
import { showRemoteToast } from '@/lib/toast'
import { VAULTMENU_FILL_EVENT, type VaultMenuFill } from '@/lib/vaultMenu'
import { unlockProblem } from '@/lib/vaultRecovery'

const EARLY_CLICK_MS = 500

export function AddressCardSurface() {
  const { data: state } = useBrowserState()
  useThemeApply(state?.settings)
  useWheelStepsAside('addresses')

  const [offer, setOffer] = useState<AddressOffer | null>(null)
  const [busy, setBusy] = useState(false)
  const shownAt = useRef(0)

  useEffect(() => {
    const unlistens: (() => void)[] = []
    let cancelled = false
    const label = getCurrentWindow().label
    const track = (pending: Promise<() => void>) =>
      void pending.then((fn) => (cancelled ? fn() : unlistens.push(fn)))
    track(
      listen<AddressOffer>(ADDRESSES_SHOW_EVENT, (event) => {
        if (event.payload.window !== label) return
        setOffer(event.payload)
        shownAt.current = performance.now()
      }),
    )
    track(
      listen<CardPlaced>(CARD_PLACED_EVENT, (event) => {
        if (event.payload.name === 'addresses' && event.payload.window === label) {
          shownAt.current = performance.now()
        }
      }),
    )
    void emit(ADDRESSES_READY_EVENT, label)
    return () => {
      cancelled = true
      unlistens.forEach((fn) => fn())
    }
  }, [])

  const early = () => performance.now() - shownAt.current < EARLY_CLICK_MS

  const closeCard = (tabId: AddressOffer['tabId'], picked: boolean) => {
    setOffer(null)
    const close: AddressesClose = { window: getCurrentWindow().label, tabId, picked }
    void emit(ADDRESSES_CLOSE_EVENT, close)
  }

  const unlock = async () => {
    if (!offer || busy || early()) return
    setBusy(true)
    try {
      const label = getCurrentWindow().label
      const status = await commands.vaultStatus().catch(() => null)
      if (status?.password) {
        const request: VaultMenuFill = { window: label, tab: offer.tabId }
        void emit(VAULTMENU_FILL_EVENT, request)
        closeCard(offer.tabId, false)
        return
      }
      const unlocked = await commands.vaultUnlock().catch((error: unknown) => {
        void showRemoteToast(label, unlockProblem(error))
        return false
      })
      if (unlocked) void emit(ADDRESSES_RELOAD_EVENT, label)
    } finally {
      setBusy(false)
    }
  }

  const choose = async (id: number) => {
    if (!offer || busy || early()) return
    setBusy(true)
    try {
      await commands.addressFill(offer.tabId, id, offer.token)
      closeCard(offer.tabId, true)
      void commands.focusWebview(`tab-${offer.tabId}`).catch(() => undefined)
    } catch (error: unknown) {
      void showRemoteToast(getCurrentWindow().label, errorMessage(error))
    } finally {
      setBusy(false)
    }
  }

  if (!offer) return null

  return (
    <div className={cx('kb-logins-card', { 'glass-menus': menusGlass(state?.settings) })} role="menu">
      <div className="kb-logins-head">Saved addresses</div>
      <div className="kb-addresses-note">{fillsLine(offer.kinds)}</div>
      <div className="kb-logins-list">
        {offer.locked ? (
          <button
            type="button"
            role="menuitem"
            className="kb-vaultmenu-row kb-logins-row"
            disabled={busy}
            title="Your addresses are kept in your vault"
            onClick={() => void unlock()}
          >
            <span className="kb-vaultmenu-ico" aria-hidden>
              <LockIcon />
            </span>
            <span className="kb-vaultmenu-user">Unlock to access your addresses</span>
          </button>
        ) : (
          offer.rows.map((row) => (
            <button
              key={row.id}
              type="button"
              role="menuitem"
              className="kb-vaultmenu-row kb-logins-row"
              disabled={busy}
              title={row.detail || undefined}
              onClick={() => void choose(row.id)}
            >
              <span className="kb-vaultmenu-ico" aria-hidden>
                <PinGlyph />
              </span>
              <span className="kb-vaultmenu-user">
                {row.title}
                {row.detail && <span className="kb-addresses-detail"> · {row.detail}</span>}
              </span>
            </button>
          ))
        )}
      </div>
    </div>
  )
}

function PinGlyph() {
  return (
    <svg
      width={16}
      height={16}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="M20 10c0 5-5.5 10.2-7.4 11.8a1 1 0 0 1-1.2 0C9.5 20.2 4 15 4 10a8 8 0 0 1 16 0" />
      <circle cx="12" cy="10" r="3" />
    </svg>
  )
}
