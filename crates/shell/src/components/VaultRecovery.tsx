import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { errorMessage } from '@/lib/errors'
import { shortKey } from '@/lib/vaultRecovery'

const VAULT_RECOVERY_KEY = ['vault-recovery-key'] as const

function useRecoveryKey() {
  return useQuery({
    queryKey: VAULT_RECOVERY_KEY,
    queryFn: () => commands.vaultRecoveryKey(),
    staleTime: 0,
  })
}

function Failure({ error }: { error: unknown }) {
  if (!error) return null
  return (
    <p className="kb-backup-status is-error" role="alert">
      {errorMessage(error)}
    </p>
  )
}

interface VaultRescueProps {
  onDone: () => void
}

/** Ways back in when the vault's key cannot be loaded. */
export function VaultRescue({ onDone }: VaultRescueProps) {
  const { data: recoveryKey, error: copyError } = useRecoveryKey()
  const [passphrase, setPassphrase] = useState('')
  const [confirmReset, setConfirmReset] = useState(false)
  const [partial, setPartial] = useState<{ opened: number; total: number } | null>(null)
  const recover = useMutation({
    mutationFn: (force: boolean) => commands.vaultRecover(passphrase || null, force),
    onMutate: () => setPartial(null),
    onSettled: (outcome) => {
      if (outcome?.kind !== 'partial') setPassphrase('')
    },
    onSuccess: (outcome) => {
      if (outcome.kind === 'recovered') onDone()
      if (outcome.kind === 'partial') setPartial({ opened: outcome.opened, total: outcome.total })
    },
  })
  const reset = useMutation({
    mutationFn: () => commands.vaultReset(),
    onSuccess: (done) => {
      if (done) onDone()
    },
  })

  return (
    <>
      {recoveryKey && (
        <>
          <div className="row">
            <div className="l">
              <span>Recover with your private key</span>
              <span>Choose the private key for {shortKey(recoveryKey)}.</span>
            </div>
          </div>
          <div className="kb-ua-add">
            <input
              type="password"
              className="kb-text-input"
              placeholder="Passphrase, if the key has one"
              value={passphrase}
              onChange={(e) => setPassphrase(e.currentTarget.value)}
              aria-label="Private key passphrase"
              autoComplete="off"
            />
            <button
              type="button"
              className="kb-btn"
              onClick={() => recover.mutate(false)}
              disabled={recover.isPending}
            >
              Choose key…
            </button>
          </div>
          {partial && (
            <div className="row">
              <div className="l">
                <span>
                  This key opens {partial.opened} of {partial.total} saved passwords
                </span>
                <span>
                  The other {partial.total - partial.opened} were sealed with a different key and
                  stay unreadable. Restoring keeps the {partial.opened} it opens.
                </span>
              </div>
              <button
                type="button"
                className="kb-btn"
                onClick={() => recover.mutate(true)}
                disabled={recover.isPending}
              >
                Restore anyway…
              </button>
            </div>
          )}
        </>
      )}
      <div className="row">
        <div className="l">
          <span>Start over</span>
          <span>Erase every saved password and set up a new vault.</span>
        </div>
        <button
          type="button"
          className="kb-btn is-danger"
          onClick={() => setConfirmReset(true)}
          disabled={reset.isPending}
        >
          Erase…
        </button>
      </div>
      <Failure error={recover.error ?? reset.error ?? copyError} />
      <ConfirmDialog
        open={confirmReset}
        title="Erase every saved password?"
        message="Wis deletes your saved passwords, the vault's key and its recovery copy, then sets up a new, empty vault. This can't be undone. Set recovery up again afterwards."
        confirmLabel="Erase"
        danger
        onConfirm={() => {
          setConfirmReset(false)
          reset.mutate()
        }}
        onCancel={() => setConfirmReset(false)}
      />
    </>
  )
}

/** The public key the vault's key is sealed to, so a lost key can be recovered. */
export function RecoveryKeyRow() {
  const qc = useQueryClient()
  const { data: recoveryKey, error: copyError } = useRecoveryKey()
  const [input, setInput] = useState('')
  const [confirmRemove, setConfirmRemove] = useState(false)
  const refresh = () => qc.invalidateQueries({ queryKey: VAULT_RECOVERY_KEY })
  const save = useMutation({
    mutationFn: () => commands.vaultSetRecoveryKey(input),
    onSuccess: () => {
      setInput('')
      void refresh()
    },
  })
  const remove = useMutation({
    mutationFn: () => commands.vaultRemoveRecoveryKey(),
    onSuccess: () => void refresh(),
  })
  const canSave = input.trim() !== '' && !save.isPending

  return (
    <>
      <div className="row">
        <div className="l">
          <span>Recovery key</span>
          <span>
            {recoveryKey
              ? `Sealed to ${shortKey(recoveryKey)}. If this device can't open your vault, its private key brings your passwords back.`
              : "Paste an age or SSH public key whose private half you keep off this device or behind a passphrase. If this device ever can't open your vault, that key brings your passwords back."}
          </span>
        </div>
        {recoveryKey && (
          <button
            type="button"
            className="kb-btn"
            onClick={() => setConfirmRemove(true)}
            disabled={remove.isPending}
          >
            Remove
          </button>
        )}
      </div>
      {!recoveryKey && (
        <div className="kb-ua-add">
          <input
            type="text"
            className="kb-text-input"
            placeholder="age1… or ssh-ed25519 AAAA…"
            value={input}
            onChange={(e) => setInput(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' && canSave) save.mutate()
            }}
            aria-label="Recovery public key"
            spellCheck={false}
            autoComplete="off"
          />
          <button type="button" className="kb-btn" onClick={() => save.mutate()} disabled={!canSave}>
            Save
          </button>
        </div>
      )}
      <Failure error={save.error ?? remove.error ?? copyError} />
      <ConfirmDialog
        open={confirmRemove}
        title="Remove the recovery key?"
        message="If this device then can't open your vault, your saved passwords can't be brought back."
        confirmLabel="Remove"
        danger
        onConfirm={() => {
          setConfirmRemove(false)
          remove.mutate()
        }}
        onCancel={() => setConfirmRemove(false)}
      />
    </>
  )
}
