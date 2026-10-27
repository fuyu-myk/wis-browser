import { useEffect, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { Card, UserSettings, UserSettingsPatch } from '@/bindings/types'
import { CardForm } from '@/features/vault/components/CardForm'
import { Switch } from '@/components/controls'
import { useVaultStatus } from '@/features/vault/hooks/useVaultStatus'
import { cardSummary, emptyCard } from '@/features/vault/lib/cards'
import { errorMessage } from '@/lib/errors'
import { XIcon } from '@/lib/icons'

const CARD_LIST_KEY = ['card-list'] as const

interface CardsSectionProps {
  settings: UserSettings
  onUpdate: (patch: UserSettingsPatch) => void
}

interface Editing {
  id: number | null
  last4: string | null
  card: Card
}

/** Settings > Payment cards: saved cards, sealed in the password vault, never with a security code. */
export function CardsSection({ settings, onUpdate }: CardsSectionProps) {
  const qc = useQueryClient()
  const { data: status } = useVaultStatus()
  const unlocked = status?.unlocked ?? false
  const [editing, setEditing] = useState<Editing | null>(null)

  const { data: saved = [] } = useQuery({
    queryKey: CARD_LIST_KEY,
    queryFn: () => commands.cardList(),
    enabled: unlocked,
    staleTime: 0,
  })

  useEffect(() => {
    if (unlocked) return
    qc.removeQueries({ queryKey: CARD_LIST_KEY })
    setEditing(null)
  }, [unlocked, qc])

  const refresh = () => qc.invalidateQueries({ queryKey: CARD_LIST_KEY })
  const open = useMutation({
    mutationFn: async ({ id, last4 }: { id: number; last4: string }) => {
      const details = await commands.cardDetails(id)
      return { id, last4, card: { ...details, number: '' } }
    },
    onSuccess: setEditing,
  })
  const save = useMutation({
    mutationFn: (entry: Editing) => commands.cardSave(entry.id, entry.card),
    onSuccess: () => {
      setEditing(null)
      void refresh()
    },
  })
  const remove = useMutation({
    mutationFn: (id: number) => commands.cardDelete(id),
    onSuccess: () => void refresh(),
  })
  const failure = open.error ?? save.error ?? remove.error
  const now = new Date()

  if (!unlocked) return null

  return (
    <section id="cards" className="sec">
      <div className="sec-h">Payment cards</div>
      <div className="row">
        <div className="l">
          <span>Save and fill payment cards</span>
          <span>
            Offer your saved cards at card fields on secure pages. Each fill asks for Touch ID or your vault password,
            and security codes are never saved.
          </span>
        </div>
        <Switch
          checked={settings.card_autofill}
          onChange={(on) => onUpdate({ card_autofill: on })}
          label="Save and fill payment cards"
        />
      </div>

      {editing ? (
        <CardForm
          value={editing.card}
          last4={editing.last4}
          busy={save.isPending}
          onChange={(card) => setEditing({ ...editing, card })}
          onSave={() => save.mutate(editing)}
          onCancel={() => {
            setEditing(null)
            save.reset()
          }}
        />
      ) : (
        <div className="row">
          <div className="l">
            <span>{saved.length === 0 ? 'No saved cards yet' : 'Another card'}</span>
            <span>Add one for easier checkout.</span>
          </div>
          <button
            type="button"
            className="kb-btn"
            onClick={() => setEditing({ id: null, last4: null, card: emptyCard(now) })}
          >
            Add card
          </button>
        </div>
      )}
      {saved.length > 0 && (
        <div className="kb-vault-panel">
          <ul className="kb-vault-list">
            {saved.map((card) => {
              const { title, detail } = cardSummary(card, now)
              return (
                <li key={card.id} className="kb-vault-row">
                  <div className="kb-vault-id">
                    <span className="kb-vault-host">{title}</span>
                    <span className="kb-vault-user">{detail}</span>
                  </div>
                  <button
                    type="button"
                    className="kb-vault-action"
                    onClick={() => open.mutate({ id: card.id, last4: card.last4 })}
                    disabled={open.isPending}
                    aria-label={`Edit ${title}`}
                  >
                    Edit
                  </button>
                  <button
                    type="button"
                    className="kb-vault-del"
                    onClick={() => remove.mutate(card.id)}
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
      {failure ? (
        <p className="kb-backup-status is-error" role="alert">
          {errorMessage(failure)}
        </p>
      ) : null}
    </section>
  )
}
