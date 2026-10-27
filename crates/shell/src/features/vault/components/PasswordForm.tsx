import { useState } from 'react'
import { useMutation } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import { cx } from '@/lib/cx'
import { errorMessage } from '@/lib/errors'

/** Mirrors the clamp in `wis_secrets::generate_password`. */
const PW_MIN_LENGTH = 8
const PW_MAX_LENGTH = 128
const PW_DEFAULT_LENGTH = 20

/**
 * Class sizes mirroring `wis_secrets::password`. Used only for the strength
 * hint.
 */
const PW_CLASS_SIZES = { lowercase: 26, uppercase: 26, digits: 10, symbols: 14 }

/** Below this an offline attack on a stolen hash is within reach. */
const PW_WEAK_BITS = 60

/** How much entropy a recipe yields, for the hint beside the button. */
function entropyBits(
  length: number,
  uppercase: boolean,
  digits: boolean,
  symbols: boolean,
): number {
  let alphabet = PW_CLASS_SIZES.lowercase
  if (uppercase) alphabet += PW_CLASS_SIZES.uppercase
  if (digits) alphabet += PW_CLASS_SIZES.digits
  if (symbols) alphabet += PW_CLASS_SIZES.symbols
  return Math.round(Math.log2(alphabet) * length)
}

/** The typed length as a number the generator will accept. */
function clampLength(raw: string): number {
  const parsed = Number.parseInt(raw, 10)
  if (Number.isNaN(parsed)) return PW_DEFAULT_LENGTH
  return Math.min(Math.max(parsed, PW_MIN_LENGTH), PW_MAX_LENGTH)
}

interface PasswordFormProps {
  onSaved: () => void
  onCancel: () => void
}

export function PasswordForm({ onSaved, onCancel }: PasswordFormProps) {
  const [host, setHost] = useState('')
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [visible, setVisible] = useState(false)
  const [lengthDraft, setLengthDraft] = useState(String(PW_DEFAULT_LENGTH))
  const [uppercase, setUppercase] = useState(true)
  const [digits, setDigits] = useState(true)
  const [symbols, setSymbols] = useState(true)

  const save = useMutation({
    mutationFn: () => commands.vaultSave(host.trim(), username.trim(), password),
    onSuccess: onSaved,
  })

  const bits = entropyBits(clampLength(lengthDraft), uppercase, digits, symbols)

  const generate = async () => {
    const generated = await commands
      .generatePassword(clampLength(lengthDraft), uppercase, digits, symbols)
      .catch(() => null)
    if (generated === null) return
    setPassword(generated)
    setVisible(true)
  }

  const canSave = host.trim() !== '' && username.trim() !== '' && password !== '' && !save.isPending

  return (
    <form
      className="kb-address-form"
      onSubmit={(e) => {
        e.preventDefault()
        if (canSave) save.mutate()
      }}
    >
      <label className="kb-address-field is-wide">
        <span>Site</span>
        <input
          type="text"
          className="kb-text-input"
          value={host}
          placeholder="example.com"
          autoComplete="off"
          spellCheck={false}
          autoCapitalize="off"
          onChange={(e) => setHost(e.currentTarget.value)}
        />
      </label>
      <label className="kb-address-field">
        <span>Username</span>
        <input
          type="text"
          className="kb-text-input"
          value={username}
          autoComplete="off"
          spellCheck={false}
          autoCapitalize="off"
          onChange={(e) => setUsername(e.currentTarget.value)}
        />
      </label>
      <div className="kb-address-field">
        <label htmlFor="kb-password-new">Password</label>
        <div className="kb-password-secret">
          <input
            id="kb-password-new"
            type={visible ? 'text' : 'password'}
            className="kb-text-input"
            value={password}
            autoComplete="off"
            spellCheck={false}
            autoCapitalize="off"
            onChange={(e) => setPassword(e.currentTarget.value)}
          />
          <button
            type="button"
            className="kb-vault-action"
            onClick={() => setVisible((v) => !v)}
            aria-pressed={visible}
          >
            {visible ? 'Hide' : 'Show'}
          </button>
        </div>
      </div>
      <div className="kb-address-field is-wide">
        <span>Generate a strong password</span>
        <div className="kb-vault-gen">
          <input
            type="number"
            className="kb-vault-len"
            min={PW_MIN_LENGTH}
            max={PW_MAX_LENGTH}
            value={lengthDraft}
            onChange={(e) => setLengthDraft(e.target.value)}
            aria-label="Generated password length"
          />
          <ClassChip label="A-Z" title="Uppercase letters" on={uppercase} onToggle={setUppercase} />
          <ClassChip label="0-9" title="Digits" on={digits} onToggle={setDigits} />
          <ClassChip label="!#$" title="Symbols" on={symbols} onToggle={setSymbols} />
          <button type="button" className="kb-btn" onClick={() => void generate()}>
            Generate
          </button>
          <span
            className={cx('kb-vault-gen-label', { 'is-weak': bits < PW_WEAK_BITS })}
            title={
              bits < PW_WEAK_BITS
                ? 'Short enough to be worth attacking offline. Add length or classes.'
                : 'Guessing entropy of a password from these settings.'
            }
          >
            {bits} bits
          </span>
        </div>
      </div>
      {save.isError && (
        <p className="kb-backup-status is-error" role="alert">
          {errorMessage(save.error)}
        </p>
      )}
      <div className="kb-address-actions">
        <button type="button" className="kb-btn" onClick={onCancel}>
          Cancel
        </button>
        <button type="submit" className="kb-btn" disabled={!canSave}>
          Save password
        </button>
      </div>
    </form>
  )
}

interface ClassChipProps {
  label: string
  title: string
  on: boolean
  onToggle: (next: boolean) => void
}

/** One character class the generator may draw from. */
function ClassChip({ label, title, on, onToggle }: ClassChipProps) {
  return (
    <button
      type="button"
      className={cx('kb-vault-chip', { 'is-on': on })}
      title={title}
      aria-label={title}
      aria-pressed={on}
      onClick={() => onToggle(!on)}
    >
      {label}
    </button>
  )
}
