import { cardSummary } from '@/features/vault/lib/cards'
import type { PendingCard } from '@/features/vault/lib/vaultMenu'

interface CardSavePromptProps {
  card: PendingCard
  busy: boolean
  onSave: () => void
  onDismiss: () => void
}

/** Card save prompt in the key-button menu, after a page submitted a user-typed card number. */
export function CardSavePrompt({ card, busy, onSave, onDismiss }: CardSavePromptProps) {
  const { title, detail } = cardSummary({ ...card, id: 0 }, new Date())

  return (
    <div className="kb-vaultmenu-save">
      <div className="kb-vaultmenu-save-text">
        <div className="kb-vaultmenu-save-title">Save card?</div>
        <div className="kb-vaultmenu-save-sub">{[title, detail].join(' · ')}</div>
        {card.name && <div className="kb-vaultmenu-save-sub">{card.name}</div>}
        <div className="kb-vaultmenu-save-sub">The security code is never saved.</div>
      </div>
      <div className="kb-vaultmenu-save-actions">
        <button type="button" className="kb-vaultmenu-btn" onClick={onDismiss} disabled={busy}>
          Not now
        </button>
        <button type="button" className="kb-vaultmenu-btn kb-vaultmenu-btn-primary" onClick={onSave} disabled={busy}>
          Save
        </button>
      </div>
    </div>
  )
}
