import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import { ChangeVaultPassword, VaultPasswordUnlock } from '@/features/vault/components/VaultPassword'
import { RecoveryKeyRow, VaultRescue } from '@/features/vault/components/VaultRecovery'
import { useVaultStatus, VAULT_STATUS_KEY } from '@/features/vault/hooks/useVaultStatus'
import { noteUnlocked, VAULT_SECTION } from '@/features/vault/lib/vaultSections'
import { VaultLockGlyph } from '@/features/vault/components/VaultLockGlyph'
import { SettingsGroup } from '@/components/SettingsGroup'
import { errorMessage } from '@/lib/errors'
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

  const lockedText = !passwordMode
    ? `Unlock with ${BIOMETRIC_NAME} to see your passwords, addresses and cards. They are encrypted on this device.`
    : hasKey
      ? 'Unlock with your vault password to see your passwords, addresses and cards. They are encrypted on this device.'
      : "Choose a vault password. Wis can't recover it for you, please set up a recovery key once unlocked."

  const refresh = () => qc.invalidateQueries({ queryKey: VAULT_STATUS_KEY })
  const unlock = useMutation({
    mutationFn: async (password?: string) => {
      if (!passwordMode) return commands.vaultUnlock()
      const typed = password ?? ''
      await (hasKey ? commands.vaultUnlockPassword(typed) : commands.vaultCreatePassword(typed))
      return true
    },
    onSuccess: () => {
      noteUnlocked()
      void refresh()
    },
  })
  const lock = useMutation({ mutationFn: () => commands.vaultLock(), onSuccess: refresh })

  return (
    <section id={VAULT_SECTION} className="sec">
      <h2 className="sec-h">Vault</h2>

      {!supported ? (
        <p className="kb-vault-msg">
          The password vault is not available on this platform.
        </p>
      ) : (
        <SettingsGroup>
          <div className="row kb-vault-status">
            <VaultLockGlyph open={unlocked} />
            <div className="l">
              <span>{unlocked ? 'Vault unlocked' : hasKey ? 'Your vault is locked' : 'Set up your vault'}</span>
              <span>
                {unlocked
                  ? 'Passwords, addresses and cards are encrypted on this device.'
                  : lockedText}
              </span>
            </div>
            {unlocked ? (
              <button type="button" className="kb-btn" onClick={() => lock.mutate()}>
                Lock
              </button>
            ) : (
              !passwordMode && (
                <button
                  type="button"
                  className="kb-btn"
                  onClick={() => unlock.mutate(undefined)}
                  disabled={unlock.isPending}
                >
                  {hasKey ? 'Unlock' : 'Set up'}
                </button>
              )
            )}
          </div>
          {!unlocked && passwordMode && (
            <VaultPasswordUnlock
              hasKey={hasKey}
              busy={unlock.isPending}
              onSubmit={(password) => unlock.mutate(password)}
            />
          )}
          {!unlocked && unlock.isError && (
            <p className="kb-backup-status is-error" role="alert">
              {errorMessage(unlock.error)}
            </p>
          )}
          {!unlocked && passwordMode && hasKey && !rescueOpen && (
            <button type="button" className="kb-vault-forgot" onClick={() => setRescueOpen(true)}>
              Forgot your password?
            </button>
          )}
          {!unlocked && (passwordMode ? rescueOpen || (!hasKey && unlock.isError) : unlock.isError) && (
            <VaultRescue
              passwordMode={passwordMode}
              onDone={() => {
                unlock.reset()
                setRescueOpen(false)
                void refresh()
              }}
            />
          )}
          {unlocked && <RecoveryKeyRow />}
          {unlocked && passwordMode && <ChangeVaultPassword />}
        </SettingsGroup>
      )}
    </section>
  )
}
