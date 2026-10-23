import type { SavedAddress } from '@/bindings/types'
import { addressSummary } from '@/features/vault/lib/addresses'
import type { PendingAddress } from '@/features/vault/lib/vaultMenu'

interface AddressSavePromptProps {
  address: PendingAddress
  /** A saved address at the same place, which the submitted one would update. */
  match: SavedAddress | null
  busy: boolean
  onSave: () => void
  onUpdate: () => void
  onDismiss: () => void
}

/** "Save address?" in the key-button menu, or "Update address?" when one is saved at the same place. */
export function AddressSavePrompt({ address, match, busy, onSave, onUpdate, onDismiss }: AddressSavePromptProps) {
  const typed = addressSummary(address.address, navigator.language)
  const rest = [
    ...address.address.street_address.split('\n').slice(1),
    address.address.organization,
    address.address.tel,
    address.address.email,
  ].filter((part) => part && part !== typed.title)
  const saved = match ? addressSummary(match.address, navigator.language) : null

  return (
    <div className="kb-vaultmenu-save">
      <div className="kb-vaultmenu-save-text">
        <div className="kb-vaultmenu-save-title">{saved ? 'Update address?' : 'Save address?'}</div>
        <div className="kb-vaultmenu-save-sub">{[typed.title, typed.detail].filter(Boolean).join(' · ')}</div>
        {rest.length > 0 && <div className="kb-vaultmenu-save-sub">{rest.join(' · ')}</div>}
        {saved && (
          <div className="kb-vaultmenu-save-sub">
            Replaces what changed in {[saved.title, saved.detail].filter(Boolean).join(' · ')}
          </div>
        )}
      </div>
      <div className="kb-vaultmenu-save-actions">
        <button type="button" className="kb-vaultmenu-btn" onClick={onDismiss} disabled={busy}>
          Not now
        </button>
        {saved && (
          <button type="button" className="kb-vaultmenu-btn" onClick={onSave} disabled={busy}>
            Save
          </button>
        )}
        <button
          type="button"
          className="kb-vaultmenu-btn kb-vaultmenu-btn-primary"
          onClick={saved ? onUpdate : onSave}
          disabled={busy}
        >
          {saved ? 'Update' : 'Save'}
        </button>
      </div>
    </div>
  )
}
