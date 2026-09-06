import { useMemo, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { cx } from '@/lib/cx'
import { LockIcon, SearchIcon, XIcon } from '@/lib/icons'
import { isMacOS } from '@/lib/platform'

const VAULT_STATUS_KEY = ['vault-status'] as const
const VAULT_LIST_KEY = ['vault-list'] as const

// The OS biometric the vault gate uses on this platform.
const BIOMETRIC_NAME = isMacOS() ? 'Touch ID' : 'Windows Hello'

interface Status {
  kind: 'ok' | 'err'
  text: string
}

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

function errorMessage(error: unknown): string {
  if (typeof error === 'string') return error
  if (error && typeof error === 'object' && 'message' in error) {
    const message = (error as { message?: unknown }).message
    if (typeof message === 'string') return message
  }
  return 'unknown error'
}

/**
 * Settings → Passwords: a biometric-gated vault (Touch ID / Windows Hello).
 * Locked by default; once unlocked, lists saved logins with reveal/copy/delete
 * and an inline add form. Passwords are fetched (decrypted) only on an explicit
 * reveal/copy.
 */
export function PasswordsSection() {
  const qc = useQueryClient()
  const [search, setSearch] = useState('')
  const [csvBusy, setCsvBusy] = useState(false)
  const [csvStatus, setCsvStatus] = useState<Status | null>(null)
  const [confirmExport, setConfirmExport] = useState(false)

  const { data: status } = useQuery({
    queryKey: VAULT_STATUS_KEY,
    queryFn: () => commands.vaultStatus(),
    staleTime: 0,
    refetchOnMount: 'always',
  })
  const supported = status?.supported ?? false
  const unlocked = status?.unlocked ?? false
  const hasKey = status?.has_master_key ?? false

  const { data: creds = [] } = useQuery({
    queryKey: VAULT_LIST_KEY,
    queryFn: () => commands.vaultList(),
    enabled: unlocked,
    staleTime: 0,
  })

  const refreshAll = () => {
    qc.invalidateQueries({ queryKey: VAULT_STATUS_KEY })
    qc.invalidateQueries({ queryKey: VAULT_LIST_KEY })
  }

  const unlock = useMutation({ mutationFn: () => commands.vaultUnlock(), onSuccess: refreshAll })
  const lock = useMutation({ mutationFn: () => commands.vaultLock(), onSuccess: refreshAll })
  const del = useMutation({
    mutationFn: (id: number) => commands.vaultDelete(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: VAULT_LIST_KEY }),
  })

  const importCsv = async () => {
    setCsvStatus(null)
    setCsvBusy(true)
    try {
      const stats = await commands.vaultImportCsv()
      if (!stats) return
      refreshAll()
      const skipped =
        stats.skipped > 0
          ? ` (${stats.skipped} row${stats.skipped === 1 ? '' : 's'} skipped)`
          : ''
      setCsvStatus({
        kind: 'ok',
        text:
          stats.imported > 0
            ? `Imported ${stats.imported} password${stats.imported === 1 ? '' : 's'}${skipped}.`
            : `No usable passwords found in that file${skipped}.`,
      })
    } catch (error: unknown) {
      setCsvStatus({ kind: 'err', text: errorMessage(error) })
    } finally {
      setCsvBusy(false)
    }
  }

  const exportCsv = async () => {
    setCsvStatus(null)
    setCsvBusy(true)
    try {
      const written = await commands.vaultExportCsv()
      refreshAll()
      if (written) {
        setCsvStatus({
          kind: 'ok',
          text: 'Passwords exported. Delete the file once you are done with it.',
        })
      }
    } catch (error: unknown) {
      setCsvStatus({ kind: 'err', text: errorMessage(error) })
    } finally {
      setCsvBusy(false)
    }
  }

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return creds
    return creds.filter(
      (c) => c.host.toLowerCase().includes(q) || c.username.toLowerCase().includes(q),
    )
  }, [creds, search])

  return (
    <section id="passwords" className="sec">
      <div className="sec-h">Passwords</div>

      {!supported ? (
        <p className="kb-vault-msg">
          The password vault is not available on this platform.
        </p>
      ) : !unlocked ? (
        <div className="kb-vault-locked">
          <span className="kb-vault-lock-icon">
            <LockIcon />
          </span>
          <div className="kb-vault-locked-text">
            <span>{hasKey ? 'Your vault is locked' : 'Set up your password vault'}</span>
            <span>Unlocked with {BIOMETRIC_NAME}. Passwords are encrypted on this device.</span>
          </div>
          <button
            type="button"
            className="kb-btn"
            onClick={() => unlock.mutate()}
            disabled={unlock.isPending}
          >
            {hasKey ? 'Unlock' : 'Set up'}
          </button>
        </div>
      ) : (
        <>
          <div className="row">
            <div className="l">
              <span>Saved passwords</span>
              <span>Stored encrypted; revealed only with your approval.</span>
            </div>
            <button type="button" className="kb-btn" onClick={() => lock.mutate()}>
              Lock
            </button>
          </div>

          <div className="row">
            <div className="l">
              <span>Import from CSV</span>
              <span>
                Bring passwords from another browser or password manager (a
                Chrome, Firefox or Safari CSV export). Existing entries for the
                same site and username are updated.
              </span>
            </div>
            <button
              type="button"
              className="kb-btn"
              onClick={() => void importCsv()}
              disabled={csvBusy}
            >
              Import…
            </button>
          </div>

          <div className="row">
            <div className="l">
              <span>Export to CSV</span>
              <span>Save every password as a plain, unencrypted CSV file.</span>
            </div>
            <button
              type="button"
              className="kb-btn"
              onClick={() => setConfirmExport(true)}
              disabled={csvBusy}
            >
              Export…
            </button>
          </div>

          {csvStatus && (
            <p className={cx('kb-backup-status', { 'is-error': csvStatus.kind === 'err' })}>
              {csvStatus.text}
            </p>
          )}

          <AddPasswordForm
            onSaved={() => qc.invalidateQueries({ queryKey: VAULT_LIST_KEY })}
          />

          <div className="kb-vault-panel">
            <div className="kb-vault-search">
              <SearchIcon />
              <input
                type="text"
                placeholder="Search passwords"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                spellCheck={false}
              />
            </div>
            {filtered.length === 0 ? (
              <p className="kb-vault-empty">
                {creds.length === 0 ? 'No saved passwords yet.' : 'No matches.'}
              </p>
            ) : (
              <ul className="kb-vault-list">
                {filtered.map((c) => (
                  <CredentialRow key={c.id} id={c.id} host={c.host} username={c.username} onDelete={() => del.mutate(c.id)} />
                ))}
              </ul>
            )}
          </div>
        </>
      )}

      <ConfirmDialog
        open={confirmExport}
        title="Export passwords as a plain CSV file?"
        message="The file is not encrypted: anyone who opens it can read every saved password. Import it into its destination app directly rather than opening it in a spreadsheet, and delete it once you are done."
        confirmLabel="Export anyway"
        danger
        onConfirm={() => {
          setConfirmExport(false)
          void exportCsv()
        }}
        onCancel={() => setConfirmExport(false)}
      />
    </section>
  )
}

interface CredentialRowProps {
  id: number
  host: string
  username: string
  onDelete: () => void
}

function CredentialRow({ id, host, username, onDelete }: CredentialRowProps) {
  const [revealed, setRevealed] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)

  const reveal = async () => {
    if (revealed !== null) {
      setRevealed(null)
      return
    }
    const pw = await commands.vaultReveal(id).catch(() => null)
    if (pw !== null) setRevealed(pw)
  }

  const copy = async () => {
    const pw = await commands.vaultReveal(id).catch(() => null)
    if (pw === null) return
    try {
      await navigator.clipboard.writeText(pw)
      setCopied(true)
      setTimeout(() => setCopied(false), 1200)
    } catch {
      // Clipboard unavailable.
    }
  }

  return (
    <li className="kb-vault-row">
      <div className="kb-vault-id">
        <span className="kb-vault-host">{host}</span>
        <span className="kb-vault-user">{username}</span>
      </div>
      <code className="kb-vault-secret">{revealed ?? '••••••••'}</code>
      <button type="button" className="kb-vault-action" onClick={() => void reveal()}>
        {revealed !== null ? 'Hide' : 'Show'}
      </button>
      <button type="button" className="kb-vault-action" onClick={() => void copy()}>
        {copied ? 'Copied' : 'Copy'}
      </button>
      <button
        type="button"
        className="kb-vault-del"
        aria-label={`Delete password for ${host}`}
        title="Delete"
        onClick={onDelete}
      >
        <XIcon />
      </button>
    </li>
  )
}

function AddPasswordForm({ onSaved }: { onSaved: () => void }) {
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
    onSuccess: () => {
      setHost('')
      setUsername('')
      setPassword('')
      setVisible(false)
      onSaved()
    },
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
      className="kb-vault-add"
      onSubmit={(e) => {
        e.preventDefault()
        if (canSave) save.mutate()
      }}
    >
      <input
        className="kb-vault-input"
        type="text"
        placeholder="example.com"
        value={host}
        onChange={(e) => setHost(e.target.value)}
        spellCheck={false}
        autoCapitalize="off"
      />
      <input
        className="kb-vault-input"
        type="text"
        placeholder="Username"
        value={username}
        onChange={(e) => setUsername(e.target.value)}
        spellCheck={false}
        autoCapitalize="off"
      />
      <input
        className="kb-vault-input"
        type={visible ? 'text' : 'password'}
        placeholder="Password"
        value={password}
        onChange={(e) => setPassword(e.target.value)}
        spellCheck={false}
        autoCapitalize="off"
      />
      <button
        type="button"
        className="kb-vault-action"
        onClick={() => setVisible((v) => !v)}
        aria-pressed={visible}
      >
        {visible ? 'Hide' : 'Show'}
      </button>
      <button type="submit" className={cx('kb-btn', { 'is-primary': canSave })} disabled={!canSave}>
        Save
      </button>
      <div className="kb-vault-gen">
        <span className="kb-vault-gen-label">Generate</span>
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
