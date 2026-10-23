import { useEffect, useMemo, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { Address, UserSettings, UserSettingsPatch } from '@/bindings/types'
import { AddressForm } from '@/features/vault/components/AddressForm'
import { Switch } from '@/components/controls'
import { useVaultStatus } from '@/features/vault/hooks/useVaultStatus'
import { addressMatches, addressSummary, emptyAddress } from '@/features/vault/lib/addresses'
import { errorMessage } from '@/lib/errors'
import { SearchIcon, XIcon } from '@/lib/icons'

const ADDRESS_LIST_KEY = ['address-list'] as const

interface AddressesSectionProps {
  settings: UserSettings
  onUpdate: (patch: UserSettingsPatch) => void
}

interface Editing {
  id: number | null
  address: Address
}

/** Settings > Addresses: saved addresses, kept sealed in the password vault. */
export function AddressesSection({ settings, onUpdate }: AddressesSectionProps) {
  const qc = useQueryClient()
  const { data: status } = useVaultStatus()
  const supported = status?.supported ?? false
  const unlocked = status?.unlocked ?? false
  const [editing, setEditing] = useState<Editing | null>(null)
  const [search, setSearch] = useState('')

  const { data: saved = [] } = useQuery({
    queryKey: ADDRESS_LIST_KEY,
    queryFn: () => commands.addressList(),
    enabled: unlocked,
    staleTime: 0,
  })
  const shown = useMemo(
    () => saved.filter(({ address }) => addressMatches(address, search, navigator.language)),
    [saved, search],
  )

  useEffect(() => {
    if (unlocked) return
    qc.removeQueries({ queryKey: ADDRESS_LIST_KEY })
    setEditing(null)
    setSearch('')
  }, [unlocked, qc])

  const refresh = () => qc.invalidateQueries({ queryKey: ADDRESS_LIST_KEY })
  const save = useMutation({
    mutationFn: (entry: Editing) => commands.addressSave(entry.id, entry.address),
    onSuccess: () => {
      setEditing(null)
      void refresh()
    },
  })
  const remove = useMutation({
    mutationFn: (id: number) => commands.addressDelete(id),
    onSuccess: () => void refresh(),
  })
  const failure = save.error ?? remove.error

  return (
    <section id="addresses" className="sec">
      <div className="sec-h">Addresses</div>
      <div className="row">
        <div className="l">
          <span>Save and fill addresses</span>
          <span>Offer your saved addresses at address fields. They are encrypted in your password vault.</span>
        </div>
        <Switch
          checked={settings.address_autofill}
          onChange={(on) => onUpdate({ address_autofill: on })}
          label="Save and fill addresses"
        />
      </div>

      {!supported ? (
        <p className="kb-vault-msg">The password vault is not available on this platform.</p>
      ) : !unlocked ? (
        <p className="kb-vault-msg">Unlock your vault under Passwords to see your addresses.</p>
      ) : (
        <>
          {saved.length > 0 && (
            <div className="kb-vault-panel">
              <div className="kb-vault-search">
                <SearchIcon />
                <input
                  type="text"
                  placeholder="Search addresses"
                  aria-label="Search addresses"
                  value={search}
                  onChange={(e) => setSearch(e.currentTarget.value)}
                  spellCheck={false}
                />
              </div>
              {shown.length === 0 && <p className="kb-vault-empty">No matches.</p>}
              <ul className="kb-vault-list">
                {shown.map(({ id, address }) => {
                  const { title, detail } = addressSummary(address, navigator.language)
                  return (
                    <li key={id} className="kb-vault-row">
                      <div className="kb-vault-id">
                        <span className="kb-vault-host">{title}</span>
                        {detail && <span className="kb-vault-user">{detail}</span>}
                      </div>
                      <button
                        type="button"
                        className="kb-vault-action"
                        onClick={() => setEditing({ id, address })}
                        aria-label={`Edit ${title}`}
                      >
                        Edit
                      </button>
                      <button
                        type="button"
                        className="kb-vault-del"
                        onClick={() => remove.mutate(id)}
                        disabled={remove.isPending}
                        aria-label={`Remove ${title}`}
                        title="Remove"
                      >
                        <XIcon />
                      </button>
                    </li>
                  )
                })}
              </ul>
            </div>
          )}
          {editing ? (
            <AddressForm
              value={editing.address}
              busy={save.isPending}
              onChange={(address) => setEditing({ ...editing, address })}
              onSave={() => save.mutate(editing)}
              onCancel={() => {
                setEditing(null)
                save.reset()
              }}
            />
          ) : (
            <div className="row">
              <div className="l">
                <span>{saved.length === 0 ? 'No saved addresses yet' : 'Another address'}</span>
                <span>Add one to have it offered at address fields.</span>
              </div>
              <button
                type="button"
                className="kb-btn"
                onClick={() => setEditing({ id: null, address: emptyAddress() })}
              >
                Add address
              </button>
            </div>
          )}
          {failure ? (
            <p className="kb-backup-status is-error" role="alert">
              {errorMessage(failure)}
            </p>
          ) : null}
        </>
      )}
    </section>
  )
}
