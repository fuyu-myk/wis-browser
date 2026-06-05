import { useMemo, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import { cx } from '@/lib/cx'
import { LockIcon, SearchIcon, XIcon } from '@/lib/icons'

const VAULT_STATUS_KEY = ['vault-status'] as const
const VAULT_LIST_KEY = ['vault-list'] as const

/**
 * Settings → Passwords: a Touch ID-gated vault. Locked by default; once
 * unlocked, lists saved logins with reveal/copy/delete and an inline add form.
 * Passwords are fetched (decrypted) only on an explicit reveal/copy.
 */
export function PasswordsSection() {
  const qc = useQueryClient()
  const [search, setSearch] = useState('')

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
        <p className="kb-vault-msg">The password vault is available on macOS only.</p>
      ) : !unlocked ? (
        <div className="kb-vault-locked">
          <span className="kb-vault-lock-icon">
            <LockIcon />
          </span>
          <div className="kb-vault-locked-text">
            <span>{hasKey ? 'Your vault is locked' : 'Set up your password vault'}</span>
            <span>Unlocked with Touch ID. Passwords are encrypted on this device.</span>
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

  const save = useMutation({
    mutationFn: () => commands.vaultSave(host.trim(), username.trim(), password),
    onSuccess: () => {
      setHost('')
      setUsername('')
      setPassword('')
      onSaved()
    },
  })

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
        type="password"
        placeholder="Password"
        value={password}
        onChange={(e) => setPassword(e.target.value)}
      />
      <button type="submit" className={cx('kb-btn', { 'is-primary': canSave })} disabled={!canSave}>
        Save
      </button>
    </form>
  )
}
