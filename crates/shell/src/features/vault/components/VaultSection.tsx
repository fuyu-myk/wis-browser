import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import { ChangeVaultPassword, VaultPasswordUnlock } from '@/features/vault/components/VaultPassword'
import { RecoveryKeyRow, VaultRescue } from '@/features/vault/components/VaultRecovery'
import { useVaultStatus, VAULT_STATUS_KEY } from '@/features/vault/hooks/useVaultStatus'
import { VAULT_SECTION } from '@/features/vault/lib/vaultSections'
import { errorMessage } from '@/lib/errors'
import { LockIcon } from '@/lib/icons'
import { isMacOS } from '@/lib/platform'

const BIOMETRIC_NAME = isMacOS() ? 'Touch ID' : 'Windows Hello'

/**
 * Settings > Vault: unlocks the vault that holds passwords, addresses and
 * cards. Their sections stay hidden until it is open.
 */
export function VaultSection() {
  const qc = useQueryClient()
  const { data: status } = useVaultStatus()
  const supported = status?.supported ?? false
  const unlocked = status?.unlocked ?? false
  const hasKey = status?.has_master_key ?? false
  const passwordMode = status?.password ?? false
  const [rescueOpen, setRescueOpen] = useState(false)

  const refresh = () => qc.invalidateQueries({ queryKey: VAULT_STATUS_KEY })
  const unlock = useMutation({
    mutationFn: async (password?: string) => {
      if (!passwordMode) return commands.vaultUnlock()
      const typed = password ?? ''
      await (hasKey ? commands.vaultUnlockPassword(typed) : commands.vaultCreatePassword(typed))
      return true
    },
    onSuccess: refresh,
  })
  const lock = useMutation({ mutationFn: () => commands.vaultLock(), onSuccess: refresh })

  return (
    <section id={VAULT_SECTION} className="sec">
      <div className="sec-h">Vault</div>

      {!supported ? (
        <p className="kb-vault-msg">
          The password vault is not available on this platform.
        </p>
      ) : !unlocked ? (
        <>
          <div className="kb-vault-locked">
            <span className="kb-vault-lock-icon">
              <LockIcon />
            </span>
            <div className="kb-vault-locked-text">
              <span>{hasKey ? 'Your vault is locked' : 'Set up your vault'}</span>
              <span>
                {!passwordMode
                  ? `Unlock with ${BIOMETRIC_NAME} to see your passwords, addresses and cards. They are encrypted on this device.`
                  : hasKey
                    ? 'Unlock with your vault password to see your passwords, addresses and cards. They are encrypted on this device.'
                    : "Choose a vault password. Wis can't recover it for you, please set up a recovery key once unlocked."}
              </span>
            </div>
            {!passwordMode && (
              <button
                type="button"
                className="kb-btn"
                onClick={() => unlock.mutate(undefined)}
                disabled={unlock.isPending}
              >
                {hasKey ? 'Unlock' : 'Set up'}
              </button>
            )}
          </div>
          {passwordMode && (
            <VaultPasswordUnlock
              hasKey={hasKey}
              busy={unlock.isPending}
              onSubmit={(password) => unlock.mutate(password)}
            />
          )}
          {unlock.isError && (
            <p className="kb-backup-status is-error" role="alert">
              {errorMessage(unlock.error)}
            </p>
          )}
          {passwordMode && hasKey && !rescueOpen && (
            <button type="button" className="kb-vault-forgot" onClick={() => setRescueOpen(true)}>
              Forgot your password?
            </button>
          )}
          {(passwordMode ? rescueOpen || (!hasKey && unlock.isError) : unlock.isError) && (
            <VaultRescue
              passwordMode={passwordMode}
              onDone={() => {
                unlock.reset()
                setRescueOpen(false)
                void refresh()
              }}
            />
          )}
        </>
      ) : (
        <>
          <div className="row">
            <div className="l">
              <span>Vault unlocked</span>
              <span>Passwords, addresses and cards are encrypted on this device.</span>
            </div>
            <button type="button" className="kb-btn" onClick={() => lock.mutate()}>
              Lock
            </button>
          </div>
          <RecoveryKeyRow />
          {passwordMode && <ChangeVaultPassword />}
        </>
      )}
    </section>
  )
}
