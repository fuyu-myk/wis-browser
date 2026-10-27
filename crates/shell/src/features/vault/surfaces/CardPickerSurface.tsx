/**
 * The saved-cards card, hosted by the `overlay-cards` webview.
 *
 * Clicks just after the card appears are ignored, so a page cannot focus a
 * field under a click already on its way and have it pick a card. Every fill
 * asks Touch ID, or here the vault password where one seals the vault.
 */

import { useEffect, useRef, useState } from 'react'
import { emit, listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { commands } from '@/bindings/commands'
import { CARD_PLACED_EVENT, useWheelStepsAside, type CardPlaced } from '@/features/vault/hooks/useAnchoredCard'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useThemeApply } from '@/hooks/useThemeApply'
import {
  CARDS_CLOSE_EVENT,
  CARDS_READY_EVENT,
  CARDS_SHOW_EVENT,
  cardFillsLine,
  type CardOffer,
  type CardsClose,
} from '@/features/vault/lib/cardOffer'
import { cx } from '@/lib/cx'
import { errorMessage } from '@/lib/errors'
import { menusGlass } from '@/lib/glass'
import { showRemoteToast } from '@/lib/toast'
import { usePickerKeys } from '@/features/vault/hooks/usePickerKeys'
import { moveFocusByArrow } from '@/lib/arrowNav'
import { ADDRESS_PICK_EVENT } from '@/features/vault/lib/addressCard'

const EARLY_CLICK_MS = 500

export function CardPickerSurface() {
  const { data: state } = useBrowserState()
  useThemeApply(state?.settings)
  useWheelStepsAside('cards')

  const [offer, setOffer] = useState<CardOffer | null>(null)
  const [busy, setBusy] = useState(false)
  const [asking, setAsking] = useState<number | null>(null)
  const [password, setPassword] = useState('')
  const shownAt = useRef(0)

  useEffect(() => {
    const unlistens: (() => void)[] = []
    let cancelled = false
    const label = getCurrentWindow().label
    const track = (pending: Promise<() => void>) =>
      void pending.then((fn) => (cancelled ? fn() : unlistens.push(fn)))
    track(
      listen<CardOffer>(CARDS_SHOW_EVENT, (event) => {
        if (event.payload.window !== label) return
        setOffer(event.payload)
        setAsking(null)
        setPassword('')
        shownAt.current = performance.now()
      }),
    )
    track(
      listen<CardPlaced>(CARD_PLACED_EVENT, (event) => {
        if (event.payload.name === 'cards' && event.payload.window === label) {
          shownAt.current = performance.now()
        }
      }),
    )
    void emit(CARDS_READY_EVENT, label)
    return () => {
      cancelled = true
      unlistens.forEach((fn) => fn())
    }
  }, [])

  const early = () => performance.now() - shownAt.current < EARLY_CLICK_MS

  const closeCard = (tabId: CardOffer['tabId'], picked: boolean) => {
    setOffer(null)
    setAsking(null)
    setPassword('')
    const close: CardsClose = { window: getCurrentWindow().label, tabId, picked }
    void emit(CARDS_CLOSE_EVENT, close)
  }

  const fill = async (id: number, vaultPassword: string | null) => {
    if (!offer || busy) return
    setBusy(true)
    try {
      await commands.cardFill(offer.tabId, id, offer.token, vaultPassword)
      closeCard(offer.tabId, true)
      void commands.focusWebview(`tab-${offer.tabId}`).catch(() => undefined)
    } catch (error: unknown) {
      setPassword('')
      void showRemoteToast(getCurrentWindow().label, errorMessage(error))
    } finally {
      setBusy(false)
    }
  }

  const choose = (id: number) => {
    if (!offer || busy || early()) return
    if (offer.password) setAsking(id)
    else void fill(id, null)
  }

  usePickerKeys(ADDRESS_PICK_EVENT, offer?.tabId ?? null, () => offer && closeCard(offer.tabId, false))

  if (!offer) return null

  return (
    <div
      className={cx('kb-logins-card', { 'glass-menus': menusGlass(state?.settings) })}
      role="menu"
      onKeyDown={(e) => moveFocusByArrow(e, e.currentTarget, 'button', 'vertical')}
    >
      <div className="kb-logins-head">Saved cards</div>
      <div className="kb-addresses-note">{cardFillsLine(offer.kinds)}</div>
      <div className="kb-logins-list">
        {asking !== null ? (
          <form
            className="kb-cards-ask"
            onSubmit={(e) => {
              e.preventDefault()
              if (password) void fill(asking, password)
            }}
          >
            <input
              type="password"
              className="kb-vaultmenu-input"
              aria-label="Vault password"
              placeholder="Vault password"
              autoComplete="off"
              autoFocus
              value={password}
              onChange={(e) => setPassword(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key !== 'Escape') return
                e.preventDefault()
                setAsking(null)
              }}
            />
            <button type="submit" className="kb-vaultmenu-btn kb-vaultmenu-btn-primary" disabled={busy || !password}>
              Fill
            </button>
          </form>
        ) : (
          offer.rows.map((row) => (
            <button
              key={row.id}
              type="button"
              role="menuitem"
              className="kb-vaultmenu-row kb-logins-row"
              disabled={busy}
              title={row.detail}
              onClick={() => choose(row.id)}
            >
              <span className="kb-vaultmenu-ico" aria-hidden>
                <CardGlyph />
              </span>
              <span className="kb-vaultmenu-user">
                {row.title}
                <span className="kb-addresses-detail"> · {row.detail}</span>
              </span>
            </button>
          ))
        )}
      </div>
    </div>
  )
}

function CardGlyph() {
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
      <rect x="2" y="5" width="20" height="14" rx="2" />
      <path d="M2 10h20" />
    </svg>
  )
}
