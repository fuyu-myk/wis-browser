import { useMemo, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { PasswordForm } from '@/features/vault/components/PasswordForm'
import { Revealed, VaultActions, VaultRow } from '@/features/vault/components/VaultRow'
import { useVaultConfirm } from '@/features/vault/components/VaultPassword'
import { useVaultStatus, VAULT_STATUS_KEY } from '@/features/vault/hooks/useVaultStatus'
import { cx } from '@/lib/cx'
import { SearchIcon } from '@/lib/icons'
import { isMacOS } from '@/lib/platform'
import { showToast } from '@/lib/toast'
import { useBrowserState, BROWSER_STATE_KEY } from '@/hooks/useBrowserState'
import { Segmented } from '@/components/controls'
import type { PasswordOffer } from '@/bindings/types'
import { useAnnounce } from '@/hooks/useAnnounce'

const VAULT_LIST_KEY = ['vault-list'] as const

// The OS biometric the vault gate uses on this platform.
const BIOMETRIC_NAME = isMacOS() ? 'Touch ID' : 'Windows Hello'

interface Status {
  kind: 'ok' | 'err'
  text: string
}

function errorMessage(error: unknown): string {
  if (typeof error === 'string') return error
  if (error && typeof error === 'object' && 'message' in error) {
    const message = (error as { message?: unknown }).message
    if (typeof message === 'string') return message
  }
  return 'unknown error'
}

const OFFER_OPTIONS = [
  { value: 'ask' as const, label: 'Ask' },
  { value: 'always' as const, label: 'Always' },
  { value: 'never' as const, label: 'Never' },
] satisfies ReadonlyArray<{ value: PasswordOffer; label: string }>

/**
 * Settings → Passwords, shown while the vault is unlocked: saved logins with
 * reveal/copy/delete and an inline add form. Passwords are fetched (decrypted) only on an explicit
 * reveal/copy.
 */
export function PasswordsSection() {
  const qc = useQueryClient()
  const { data: browser } = useBrowserState()
  const workspaceId = browser?.active_workspace_id ?? null
  const [search, setSearch] = useState('')
  const [csvBusy, setCsvBusy] = useState(false)
  const [csvStatus, setCsvStatus] = useState<Status | null>(null)
  useAnnounce(csvStatus?.kind === 'ok' && csvStatus.text)
  const [confirmExport, setConfirmExport] = useState(false)
  const [adding, setAdding] = useState(false)
  const [openId, setOpenId] = useState<number | null>(null)

  const { data: status } = useVaultStatus()
  const unlocked = status?.unlocked ?? false
  const { confirm, prompt } = useVaultConfirm()

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

  const del = useMutation({
    mutationFn: (id: number) => commands.vaultDelete(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: VAULT_LIST_KEY }),
  })

  const offer = browser?.settings.password_offer ?? 'ask'
  const setOffer = useMutation({
    mutationFn: (next: PasswordOffer) => commands.updateSettings({ password_offer: next }),
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })

  const importCsv = async () => {
    setCsvStatus(null)
    const consent = await confirm()
    if (!consent) return
    setCsvBusy(true)
    try {
      const stats = await commands.vaultImportCsv(consent.password)
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
    const consent = await confirm()
    if (!consent) return
    setCsvBusy(true)
    try {
      const written = await commands.vaultExportCsv(consent.password)
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

  // Row id -> times that password appears in known breaches, from the last
  // check.
  const [breaches, setBreaches] = useState<Map<number, number> | null>(null)
  const [breachStatus, setBreachStatus] = useState<Status | null>(null)
  const checkBreaches = useMutation({
    mutationFn: async () => {
      if (workspaceId === null) throw new Error('no workspace to route the check through')
      const consent = await confirm()
      if (!consent) return null
      return commands.vaultCheckBreaches(workspaceId, consent.password)
    },
    onSuccess: (report) => {
      if (!report) return
      setBreaches(new Map(report.breached.map((found) => [found.id, found.count])))
      setBreachStatus({
        kind: report.breached.length === 0 ? 'ok' : 'err',
        text:
          report.breached.length === 0
            ? `None of your ${report.checked} saved passwords appear in known breaches.`
            : `${report.breached.length} of ${report.checked} appear in known breaches. Change those first.`,
      })
    },
    onError: (error: unknown) => {
      setBreaches(null)
      setBreachStatus({ kind: 'err', text: errorMessage(error) })
    },
  })

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return creds
    return creds.filter(
      (c) => c.host.toLowerCase().includes(q) || c.username.toLowerCase().includes(q),
    )
  }, [creds, search])

  if (!unlocked) return null

  return (
    <section id="passwords" className="sec">
      <h2 className="sec-h">Passwords</h2>
      <div className="row">
        <div className="l">
          <span>Offer a strong password on signup</span>
          <span>
            {offer === 'always'
              ? 'Filled into empty password fields without asking.'
              : offer === 'never'
                ? 'Never offered. Generate one here instead.'
                : 'Offered when a signup form takes focus.'}
          </span>
        </div>
        <Segmented
          value={offer}
          onChange={(next) => setOffer.mutate(next)}
          options={OFFER_OPTIONS}
          ariaLabel="Offer a strong password on signup"
        />
      </div>

      <div className="row">
        <div className="l">
          <span>Check for breaches</span>
          <span className={cx({ 'is-error': breachStatus?.kind === 'err' })}>
            {breachStatus?.text ??
              `Looks your passwords up in public breach data without revealing them. Each is hashed here and only the first five characters of the hash are sent. Requests follow this workspace\u2019s network route, and ${BIOMETRIC_NAME} is asked for first.`}
          </span>
        </div>
        <button
          type="button"
          className="kb-btn"
          onClick={() => checkBreaches.mutate()}
          disabled={checkBreaches.isPending || workspaceId === null}
        >
          {checkBreaches.isPending ? 'Checking\u2026' : 'Check'}
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
        <p
          className={cx('kb-backup-status', { 'is-error': csvStatus.kind === 'err' })}
          role={csvStatus.kind === 'err' ? 'alert' : undefined}
        >
          {csvStatus.text}
        </p>
      )}

      {adding ? (
        <Revealed>
          <PasswordForm
            onSaved={() => {
              setAdding(false)
              void qc.invalidateQueries({ queryKey: VAULT_LIST_KEY })
            }}
            onCancel={() => setAdding(false)}
          />
        </Revealed>
      ) : (
        <div className="row">
          <div className="l">
            <span>{creds.length === 0 ? 'No saved passwords yet' : 'Another password'}</span>
            <span>Add one for more convenient sign-ins.</span>
          </div>
          <button type="button" className="kb-btn" onClick={() => setAdding(true)}>
            Add password
          </button>
        </div>
      )}

      <div className="kb-vault-panel">
        <div className="kb-vault-search">
          <SearchIcon />
          <input
            type="text"
            placeholder="Search passwords"
            aria-label="Search passwords"
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
              <CredentialRow
                key={c.id}
                id={c.id}
                host={c.host}
                username={c.username}
                breachCount={breaches?.get(c.id) ?? null}
                open={openId === c.id}
                onToggle={() => setOpenId(openId === c.id ? null : c.id)}
                removing={del.isPending}
                onDelete={() => del.mutate(c.id)}
              />
            ))}
          </ul>
        )}
      </div>

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
      {prompt}
    </section>
  )
}

interface CredentialRowProps {
  id: number
  host: string
  username: string
  /** Times this password appears in known breaches, or null when unchecked. */
  breachCount: number | null
  open: boolean
  onToggle: () => void
  removing: boolean
  onDelete: () => void
}

function revealProblem(error: unknown): null {
  showToast(errorMessage(error))
  return null
}

function CredentialRow({ host, username, breachCount, open, onToggle, ...rest }: CredentialRowProps) {
  return (
    <VaultRow
      title={host}
      detail={username}
      badge={breachCount !== null && <span className="kb-vault-breach">Breached</span>}
      open={open}
      onToggle={onToggle}
    >
      <CredentialBody host={host} username={username} breachCount={breachCount} {...rest} />
    </VaultRow>
  )
}

/** Mounted only while the row is open. */
function CredentialBody({
  id,
  host,
  username,
  breachCount,
  removing,
  onDelete,
}: Omit<CredentialRowProps, 'open' | 'onToggle'>) {
  const [revealed, setRevealed] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)

  const reveal = async () => {
    if (revealed !== null) {
      setRevealed(null)
      return
    }
    const pw = await commands.vaultReveal(id).catch(revealProblem)
    if (pw !== null) setRevealed(pw)
  }

  const copy = async () => {
    const pw = await commands.vaultReveal(id).catch(revealProblem)
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
    <>
      <dl className="kb-vault-fields">
        <div>
          <dt>Username</dt>
          <dd>{username}</dd>
        </div>
        <div>
          <dt>Password</dt>
          <dd>
            <code className="kb-vault-secret">{revealed ?? '••••••••'}</code>
          </dd>
        </div>
        {breachCount !== null && (
          <div>
            <dt>Breaches</dt>
            <dd className="is-error">
              Seen {breachCount.toLocaleString()} times in known breaches. Consider changing it.
            </dd>
          </div>
        )}
      </dl>
      <VaultActions
        removeLabel={`Remove the password for ${host}`}
        removing={removing}
        onRemove={onDelete}
      >
        <button type="button" className="kb-vault-action" onClick={() => void reveal()}>
          {revealed !== null ? 'Hide' : 'Show'}
        </button>
        <button type="button" className="kb-vault-action" onClick={() => void copy()}>
          {copied ? 'Copied' : 'Copy'}
        </button>
      </VaultActions>
    </>
  )
}
