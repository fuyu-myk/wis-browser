import { useEffect, useMemo, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { Address, UserSettings, UserSettingsPatch } from '@/bindings/types'
import { AddressForm } from '@/features/vault/components/AddressForm'
import { Revealed, VaultActions, VaultFields, VaultRow } from '@/features/vault/components/VaultRow'
import { Switch } from '@/components/controls'
import { useVaultStatus } from '@/features/vault/hooks/useVaultStatus'
import { addressFields, addressMatches, addressSummary, emptyAddress } from '@/features/vault/lib/addresses'
import { errorMessage } from '@/lib/errors'
import { SearchIcon } from '@/lib/icons'

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
  const unlocked = status?.unlocked ?? false
  const [editing, setEditing] = useState<Editing | null>(null)
  const [openId, setOpenId] = useState<number | null>(null)
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

  if (!unlocked) return null

  const cancel = () => {
    setEditing(null)
    save.reset()
  }
  const toggle = (id: number) => {
    if (editing && editing.id !== null) cancel()
    setOpenId(openId === id ? null : id)
  }
  const form = editing && (
    <AddressForm
      value={editing.address}
      busy={save.isPending}
      onChange={(address) => setEditing({ ...editing, address })}
      onSave={() => save.mutate(editing)}
      onCancel={cancel}
    />
  )

  return (
    <section id="addresses" className="sec">
      <h2 className="sec-h">Addresses</h2>
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

      {editing?.id === null ? (
        <Revealed>{form}</Revealed>
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
                <VaultRow
                  key={id}
                  title={title}
                  detail={detail}
                  open={openId === id}
                  onToggle={() => toggle(id)}
                >
                  {editing?.id === id ? (
                    form
                  ) : (
                    <>
                      <VaultFields fields={addressFields(address, navigator.language)} />
                      <VaultActions
                        removeLabel={`Remove ${title}`}
                        removing={remove.isPending}
                        onRemove={() => remove.mutate(id)}
                      >
                        <button type="button" className="kb-vault-action" onClick={() => setEditing({ id, address })}>
                          Edit
                        </button>
                      </VaultActions>
                    </>
                  )}
                </VaultRow>
              )
            })}
          </ul>
        </div>
      )}
      {failure ? (
        <p className="kb-backup-status is-error" role="alert">
          {errorMessage(failure)}
        </p>
      ) : null}
    </section>
  )
}
