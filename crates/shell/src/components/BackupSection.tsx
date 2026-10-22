/**
 * Settings -> Backup & restore. Exports selected categories to a passphrase-
 * encrypted `.wisbackup` file (Argon2id + ChaCha20-Poly1305, done natively) and
 * restores one by merging it in. The backend returns the decrypted, re-minted
 * payload; this component applies it through the existing settings commands so
 * the merge reuses their validation and side effects.
 */

import { useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type {
  CustomTheme,
  ImportSummary,
  UserSettings,
  UserSettingsPatch,
} from '@/bindings/types'
import { Switch } from '@/components/controls'
import { useVaultConfirm } from '@/components/VaultPassword'
import { cx } from '@/lib/cx'
import { withImportedRoutes } from '@/lib/linkRoutes'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'

interface BackupSectionProps {
  settings: UserSettings
}

function errorMessage(error: unknown): string {
  if (typeof error === 'string') return error
  if (
    error !== null &&
    typeof error === 'object' &&
    'message' in error &&
    typeof (error as { message: unknown }).message === 'string'
  ) {
    return (error as { message: string }).message
  }
  return 'unknown error'
}

interface Status {
  kind: 'ok' | 'err'
  text: string
}

/** Two themes are "the same" when their name and token values match, so a
 *  re-import of a backup doesn't pile up duplicate copies. */
function sameTheme(a: CustomTheme, b: CustomTheme): boolean {
  return a.name === b.name && JSON.stringify(a.tokens) === JSON.stringify(b.tokens)
}

export function BackupSection({ settings }: BackupSectionProps) {
  const qc = useQueryClient()
  const { confirm, prompt } = useVaultConfirm()
  const [includeSettings, setIncludeSettings] = useState(true)
  const [includeProfile, setIncludeProfile] = useState(false)
  const [includeHistory, setIncludeHistory] = useState(false)
  const [includeCanvases, setIncludeCanvases] = useState(true)
  const [includeVault, setIncludeVault] = useState(false)
  const [exportPass, setExportPass] = useState('')
  const [importPass, setImportPass] = useState('')
  const [busy, setBusy] = useState(false)
  const [status, setStatus] = useState<Status | null>(null)

  const runExport = async () => {
    setStatus(null)
    if (!exportPass.trim()) {
      setStatus({ kind: 'err', text: 'Choose a passphrase to encrypt the backup.' })
      return
    }
    if (!includeSettings && !includeProfile && !includeVault) {
      setStatus({ kind: 'err', text: 'Select at least one category to back up.' })
      return
    }
    const consent = includeVault ? await confirm() : { password: null }
    if (!consent) return
    setBusy(true)
    try {
      const options = {
        settings: includeSettings,
        profile: includeProfile,
        history: includeProfile && includeHistory,
        canvases: includeProfile && includeCanvases,
        vault: includeVault,
      }
      const saved = await commands.exportBackup(exportPass, options, consent.password)
      if (saved) {
        setExportPass('')
        setStatus({
          kind: 'ok',
          text: 'Backup saved. Keep the passphrase safe as it cannot be recovered.',
        })
      }
    } catch (error: unknown) {
      setStatus({ kind: 'err', text: errorMessage(error) })
    } finally {
      setBusy(false)
    }
  }

  const runImport = async () => {
    setStatus(null)
    if (!importPass.trim()) {
      setStatus({ kind: 'err', text: "Enter the backup's passphrase." })
      return
    }
    const consent = await confirm()
    if (!consent) return
    setBusy(true)
    try {
      const summary: ImportSummary | null = await commands.importBackup(importPass, consent.password)
      if (!summary) return // cancelled
      const applied: string[] = []
      if (summary.settings) {
        const imported = summary.settings
        // Merge themes: append only those not already present by name + content
        // (the backend re-minted ids), so a self-import never duplicates. Then
        // apply the rest of the settings.
        const newThemes = imported.custom_themes.filter(
          (it) => !settings.custom_themes.some((cur) => sameTheme(cur, it)),
        )
        const mergedThemes = [...settings.custom_themes, ...newThemes]
        let active = imported.active_theme ?? null
        if (active && !mergedThemes.some((t) => t.id === active)) {
          const dup = imported.custom_themes.find((t) => t.id === active)
          active = (dup && settings.custom_themes.find((cur) => sameTheme(cur, dup))?.id) ?? null
        }
        const patch = {
          ...imported,
          custom_themes: mergedThemes,
          link_routes: withImportedRoutes(settings.link_routes, imported.link_routes),
        } as UserSettingsPatch
        await commands.updateSettings(patch)
        if (active) await commands.setActiveTheme(active)
        applied.push('settings & themes')
      }
      if (summary.workspaces_added > 0) {
        const bits = [`${summary.workspaces_added} workspace${summary.workspaces_added === 1 ? '' : 's'}`]
        if (summary.bookmarks_added > 0) bits.push(`${summary.bookmarks_added} bookmarks`)
        if (summary.history_added > 0) bits.push(`${summary.history_added} history entries`)
        if (summary.canvases_added > 0) {
          bits.push(`${summary.canvases_added} canvas${summary.canvases_added === 1 ? '' : 'es'}`)
        }
        applied.push(bits.join(' + '))
      }
      if (summary.passwords_added > 0) {
        applied.push(`${summary.passwords_added} saved password${summary.passwords_added === 1 ? '' : 's'}`)
        await qc.invalidateQueries({ queryKey: ['vault-list'] })
        await qc.invalidateQueries({ queryKey: ['vault-status'] })
      }
      await qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY })
      setImportPass('')
      setStatus({
        kind: 'ok',
        text: applied.length
          ? `Restored ${applied.join(', ')}.`
          : 'Nothing new to restore (same-named workspaces are skipped).',
      })
    } catch (error: unknown) {
      setStatus({ kind: 'err', text: errorMessage(error) })
    } finally {
      setBusy(false)
    }
  }

  return (
    <section id="backup" className="sec">
      <div className="sec-h">Backup &amp; restore</div>

      <div className="row">
        <div className="l">
          <span>Settings, shortcuts &amp; themes</span>
          <span>Layout, keyboard shortcuts, new-tab setup, and custom themes.</span>
        </div>
        <Switch
          checked={includeSettings}
          onChange={setIncludeSettings}
          label="Include settings and themes"
        />
      </div>

      <div className="row">
        <div className="l">
          <span>Workspaces, tabs &amp; bookmarks</span>
          <span>Your workspaces with their tabs, split layout, folders, and bookmarks. Imported as new workspaces; same-named ones are skipped.</span>
        </div>
        <Switch
          checked={includeProfile}
          onChange={setIncludeProfile}
          label="Include workspaces, tabs and bookmarks"
        />
      </div>

      <div className="row">
        <div className="l">
          <span>Browsing history</span>
          <span>Visited URLs for the workspaces above (travels only with them).</span>
        </div>
        <Switch
          checked={includeHistory}
          onChange={setIncludeHistory}
          label="Include browsing history"
        />
      </div>

      <div className="row">
        <div className="l">
          <span>Canvases</span>
          <span>The canvases of the workspaces above, with their elements.</span>
        </div>
        <Switch
          checked={includeCanvases}
          onChange={setIncludeCanvases}
          label="Include canvases"
        />
      </div>

      <div className="row">
        <div className="l">
          <span>Saved passwords</span>
          <span>Your vault logins, re-encrypted under your backup passphrase. Touch ID is required to include or restore them.</span>
        </div>
        <Switch
          checked={includeVault}
          onChange={setIncludeVault}
          label="Include saved passwords"
        />
      </div>

      <div className="row">
        <div className="l">
          <span>Backup passphrase</span>
          <span>Encrypts the file (Argon2id + ChaCha20-Poly1305). It cannot be recovered.</span>
        </div>
        <input
          type="password"
          className="kb-text-input"
          placeholder="passphrase"
          value={exportPass}
          autoComplete="new-password"
          onChange={(e) => setExportPass(e.currentTarget.value)}
        />
      </div>
      <div className="row">
        <div className="l">
          <span>Export backup</span>
          <span>Save an encrypted .wisbackup file to move your profile elsewhere.</span>
        </div>
        <button type="button" className="kb-btn" onClick={() => void runExport()} disabled={busy}>
          Export backup…
        </button>
      </div>

      <div className="row">
        <div className="l">
          <span>Restore passphrase</span>
          <span>The passphrase the backup file was created with.</span>
        </div>
        <input
          type="password"
          className="kb-text-input"
          placeholder="passphrase"
          value={importPass}
          autoComplete="off"
          onChange={(e) => setImportPass(e.currentTarget.value)}
        />
      </div>
      <div className="row">
        <div className="l">
          <span>Restore backup</span>
          <span>Open a .wisbackup file and merge it in. Imported items get fresh ids, your existing data is kept.</span>
        </div>
        <button type="button" className="kb-btn" onClick={() => void runImport()} disabled={busy}>
          Restore backup…
        </button>
      </div>

      {status && (
        <p className={cx('kb-backup-status', { 'is-error': status.kind === 'err' })}>
          {status.text}
        </p>
      )}
      {prompt}
    </section>
  )
}
