import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react'
import { useMutation } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import { useVaultStatus } from '@/hooks/useVaultStatus'
import { errorMessage } from '@/lib/errors'
import { newPasswordProblem } from '@/lib/vaultPassword'

interface PasswordFieldProps {
  value: string
  onChange: (value: string) => void
  label: string
  onEnter?: () => void
  autoFocus?: boolean
}

function PasswordField({ value, onChange, label, onEnter, autoFocus }: PasswordFieldProps) {
  return (
    <input
      type="password"
      className="kb-text-input"
      placeholder={label}
      aria-label={label}
      value={value}
      autoFocus={autoFocus}
      autoComplete="off"
      onChange={(e) => onChange(e.currentTarget.value)}
      onKeyDown={(e) => {
        if (e.key === 'Enter') onEnter?.()
      }}
    />
  )
}

interface NewPassword {
  password: string
  repeat: string
}

export const NO_NEW_PASSWORD: NewPassword = { password: '', repeat: '' }

interface NewPasswordFieldsProps {
  value: NewPassword
  onChange: (value: NewPassword) => void
  onEnter?: () => void
}

export function NewPasswordFields({ value, onChange, onEnter }: NewPasswordFieldsProps) {
  const problem = newPasswordProblem(value.password, value.repeat)
  return (
    <>
      <div className="kb-ua-add">
        <PasswordField
          label="New vault password"
          value={value.password}
          onChange={(password) => onChange({ ...value, password })}
          onEnter={onEnter}
        />
        <PasswordField
          label="Re-enter vault password"
          value={value.repeat}
          onChange={(repeat) => onChange({ ...value, repeat })}
          onEnter={onEnter}
        />
      </div>
      {value.password && problem && <p className="kb-backup-status">{problem}</p>}
    </>
  )
}

export function newPasswordReady(value: NewPassword): boolean {
  return newPasswordProblem(value.password, value.repeat) === null
}

interface VaultPasswordUnlockProps {
  hasKey: boolean
  busy: boolean
  onSubmit: (password: string) => void
}

/** Unlock with the vault password, or choose one when there is no vault yet. */
export function VaultPasswordUnlock({ hasKey, busy, onSubmit }: VaultPasswordUnlockProps) {
  const [password, setPassword] = useState('')
  const [chosen, setChosen] = useState(NO_NEW_PASSWORD)

  if (hasKey) {
    const unlock = () => {
      if (!password || busy) return
      onSubmit(password)
      setPassword('')
    }
    return (
      <div className="kb-ua-add">
        <PasswordField label="Vault password" value={password} onChange={setPassword} onEnter={unlock} />
        <button type="button" className="kb-btn" onClick={unlock} disabled={!password || busy}>
          Unlock
        </button>
      </div>
    )
  }

  const setUp = () => {
    if (!newPasswordReady(chosen) || busy) return
    onSubmit(chosen.password)
    setChosen(NO_NEW_PASSWORD)
  }
  return (
    <>
      <NewPasswordFields value={chosen} onChange={setChosen} onEnter={setUp} />
      <div className="kb-ua-add">
        <button
          type="button"
          className="kb-btn"
          onClick={setUp}
          disabled={!newPasswordReady(chosen) || busy}
        >
          Set up
        </button>
      </div>
    </>
  )
}

/** Change the vault password, given the current one. */
export function ChangeVaultPassword() {
  const [current, setCurrent] = useState('')
  const [chosen, setChosen] = useState(NO_NEW_PASSWORD)
  const change = useMutation({
    mutationFn: () => commands.vaultChangePassword(current, chosen.password),
    onSettled: () => setCurrent(''),
    onSuccess: () => setChosen(NO_NEW_PASSWORD),
  })
  const ready = current !== '' && newPasswordReady(chosen) && !change.isPending
  const submit = () => {
    if (ready) change.mutate()
  }

  return (
    <>
      <div className="row">
        <div className="l">
          <span>Vault password</span>
          <span>
            Wis can't recover it for you, so keep a recovery key set up in case you forget it.
          </span>
        </div>
      </div>
      <div className="kb-ua-add">
        <PasswordField label="Current password" value={current} onChange={setCurrent} onEnter={submit} />
      </div>
      <NewPasswordFields value={chosen} onChange={setChosen} onEnter={submit} />
      <div className="kb-ua-add">
        <button type="button" className="kb-btn" onClick={submit} disabled={!ready}>
          Change password
        </button>
      </div>
      {change.isSuccess && <p className="kb-backup-status">Vault password changed.</p>}
      {change.isError && (
        <p className="kb-backup-status is-error" role="alert">
          {errorMessage(change.error)}
        </p>
      )}
    </>
  )
}

/** What an action that asks again sends: the vault password where one is used. */
export interface VaultConsent {
  password: string | null
}

interface VaultConfirm {
  confirm: () => Promise<VaultConsent | null>
  prompt: ReactNode
}

/**
 * Asks for the vault password before an action that asks again, where the
 * vault uses one; resolves at once where the backend runs a biometric check.
 * `null` when the user cancels.
 */
export function useVaultConfirm(): VaultConfirm {
  const { data: status } = useVaultStatus()
  const asks = (status?.password ?? false) && (status?.has_master_key ?? false)
  const [pending, setPending] = useState<{
    resolve: (consent: VaultConsent | null) => void
  } | null>(null)

  const confirm = useCallback(
    () =>
      asks
        ? new Promise<VaultConsent | null>((resolve) => setPending({ resolve }))
        : Promise.resolve({ password: null }),
    [asks],
  )
  const finish = useCallback(
    (password: string | null) => {
      pending?.resolve(password === null ? null : { password })
      setPending(null)
    },
    [pending],
  )

  return { confirm, prompt: pending ? <VaultPasswordPrompt onDone={finish} /> : null }
}

function VaultPasswordPrompt({ onDone }: { onDone: (password: string | null) => void }) {
  const [password, setPassword] = useState('')
  const [error, setError] = useState<unknown>(null)
  const [busy, setBusy] = useState(false)
  const field = useRef<HTMLInputElement | null>(null)

  useEffect(() => {
    field.current?.focus()
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onDone(null)
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [onDone])

  const submit = async () => {
    if (!password || busy) return
    setBusy(true)
    setError(null)
    try {
      await commands.vaultUnlockPassword(password)
      onDone(password)
    } catch (err) {
      setError(err)
      setPassword('')
    } finally {
      setBusy(false)
    }
  }

  return (
    <div
      className="kb-modal-backdrop"
      role="presentation"
      onPointerDown={(e) => {
        if (e.target === e.currentTarget) onDone(null)
      }}
    >
      <div className="kb-modal" role="dialog" aria-modal="true" aria-label="Enter your vault password">
        <h2 className="kb-modal-title">Enter your vault password</h2>
        <input
          ref={field}
          type="password"
          className="kb-text-input kb-modal-field"
          aria-label="Vault password"
          autoComplete="off"
          value={password}
          onChange={(e) => setPassword(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') void submit()
          }}
        />
        {error ? (
          <p className="kb-backup-status is-error" role="alert">
            {errorMessage(error)}
          </p>
        ) : null}
        <div className="kb-modal-actions">
          <button type="button" className="kb-modal-btn" onClick={() => onDone(null)}>
            Cancel
          </button>
          <button
            type="button"
            className="kb-modal-btn is-primary"
            onClick={() => void submit()}
            disabled={!password || busy}
          >
            Continue
          </button>
        </div>
      </div>
    </div>
  )
}
