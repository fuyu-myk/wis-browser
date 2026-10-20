import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { commands } from '@/bindings/commands'
import type { InstalledExtension, SiteAccess, StagedExtension, Workspace } from '@/bindings/types'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { Segmented, Switch } from '@/components/controls'
import { errorMessage } from '@/lib/errors'
import { describeLinked, describeUpdates } from '@/lib/extensionUpdates'

interface ExtensionsSectionProps {
  installed: InstalledExtension[]
  /** The workspace whose switch the section shows. */
  workspace: Workspace | undefined
}

export function ReviewDetails({ staged }: { staged: StagedExtension }) {
  const { review } = staged
  return (
    <>
      {review.description && <span className="kb-ext-line">{review.description}</span>}
      {review.abilities.length > 0 && (
        <>
          <span className="kb-ext-line kb-ext-heading">It can:</span>
          {review.abilities.map((ability) => (
            <span key={ability} className="kb-ext-line kb-ext-item">
              {ability}
            </span>
          ))}
        </>
      )}
      {review.unsupported.length > 0 && (
        <span className="kb-ext-line kb-ext-warn">
          It also asks for {review.unsupported.join(', ')}, which Wis cannot provide. The parts
          that use them may not work.
        </span>
      )}
      {review.large_rule_set !== null && (
        <span className="kb-ext-line kb-ext-warn">
          It turns on {review.large_rule_set.toLocaleString()} blocking rules. Wis already blocks
          ads and trackers, and compiling these can use several hundred MB while the browser
          starts.
        </span>
      )}
    </>
  )
}

const STORE_ITEM_URL = 'https://chromewebstore.google.com/detail/'

const SITE_ACCESS: ReadonlyArray<{ value: SiteAccess; label: string }> = [
  { value: 'all', label: 'All sites' },
  { value: 'allowed', label: 'Sites I allow' },
]

interface GrantedRowProps {
  ext: InstalledExtension
  busy: boolean
  onTakeBack: () => void
}

function GrantedRow({ ext, busy, onTakeBack }: GrantedRowProps) {
  const { data: abilities = [] } = useQuery({
    queryKey: ['extension-granted', ext.id, ext.granted],
    queryFn: () => commands.extensionGrantedAbilities(ext.id),
  })
  if (abilities.length === 0) return null
  return (
    <div className="row kb-ext-sub">
      <div className="l">
        <span>Allowed while running</span>
        {abilities.map((ability) => (
          <span key={ability}>{ability}</span>
        ))}
      </div>
      <button
        type="button"
        className="kb-btn"
        onClick={onTakeBack}
        disabled={busy}
        aria-label={`Take back what ${ext.name} was allowed while running`}
      >
        Take back
      </button>
    </div>
  )
}

export function ExtensionsSection({ installed, workspace }: ExtensionsSectionProps) {
  const { data: supported } = useQuery({
    queryKey: ['extensions-supported'],
    queryFn: () => commands.extensionsSupported(),
    staleTime: Infinity,
  })
  const [staged, setStaged] = useState<StagedExtension | null>(null)
  const [removing, setRemoving] = useState<InstalledExtension | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [updateNote, setUpdateNote] = useState<string | null>(null)

  const run = async (work: () => Promise<void>) => {
    setBusy(true)
    setError(null)
    try {
      await work()
    } catch (err: unknown) {
      setError(errorMessage(err))
    } finally {
      setBusy(false)
    }
  }

  const choose = () =>
    run(async () => {
      setStaged(await commands.extensionChoose())
    })

  const install = (token: string) => {
    setStaged(null)
    void run(() => commands.extensionInstall(token))
  }

  const cancelReview = (token: string) => {
    setStaged(null)
    void commands.extensionDiscard(token).catch(() => undefined)
  }

  const remove = (id: string) => {
    setRemoving(null)
    void run(() => commands.extensionRemove(id))
  }

  const regular = workspace && !workspace.ephemeral ? workspace : undefined

  return (
    <section id="extensions" className="sec">
      <div className="sec-h">Extensions</div>
      <div className="row">
        <div className="l">
          <span>Add an extension</span>
          <span>
            {supported === false
              ? 'Extensions need macOS 15.4 or later, and are not available on Windows or Linux yet.'
              : 'From a Chrome Web Store page through the extensions menu, or from an unpacked folder or a .zip here. They run in your regular workspaces, and open pages reload with them.'}
          </span>
        </div>
        <button
          type="button"
          className="kb-btn"
          onClick={() => void choose()}
          disabled={busy || supported !== true}
        >
          Add extension…
        </button>
      </div>
      {installed.length > 0 && (
        <div className="row">
          <div className="l">
            <span>Updates</span>
            <span>
              {updateNote ??
                'Extensions from the Chrome Web Store are checked for updates about once a day, and the store listings of those added from a folder are looked for.'}
            </span>
          </div>
          <button
            type="button"
            className="kb-btn"
            onClick={() =>
              void run(async () => setUpdateNote(describeUpdates(await commands.extensionCheckUpdates())))
            }
            disabled={busy}
          >
            Check now
          </button>
        </div>
      )}
      {installed.map((ext) => (
        <div key={ext.id}>
          <div className="row">
            <div className="l">
              <span>{ext.name}</span>
              <span>
                Version {ext.version}
                {ext.store_id && ' · From the Chrome Web Store'}
              </span>
            </div>
            <button
              type="button"
              className="kb-btn"
              onClick={() => setRemoving(ext)}
              disabled={busy}
              aria-label={`Remove ${ext.name}`}
            >
              Remove
            </button>
          </div>
          {ext.store_suggestion && !ext.store_id && (
            <div className="row kb-ext-sub">
              <div className="l">
                <span>Get its updates from the Chrome Web Store?</span>
                <span>
                  {ext.store_suggestion.same_files
                    ? 'A store listing has this extension with the same files.'
                    : 'A store listing has an extension of this name at another version.'}{' '}
                  Open it to check if it is the one you use.
                </span>
              </div>
              <div className="kb-ext-actions">
                <button
                  type="button"
                  className="kb-btn"
                  onClick={() =>
                    workspace &&
                    void commands
                      .createTab(workspace.id, `${STORE_ITEM_URL}${ext.store_suggestion?.store_id}`)
                      .catch(() => undefined)
                  }
                  disabled={!workspace}
                >
                  Open listing
                </button>
                <button
                  type="button"
                  className="kb-btn"
                  onClick={() => {
                    const storeId = ext.store_suggestion?.store_id
                    if (!storeId) return
                    void run(async () =>
                      setUpdateNote(describeLinked(ext.name, await commands.extensionLinkStore(ext.id, storeId))),
                    )
                  }}
                  disabled={busy}
                >
                  Link
                </button>
                <button
                  type="button"
                  className="kb-btn"
                  onClick={() => {
                    const storeId = ext.store_suggestion?.store_id
                    if (storeId) void run(() => commands.extensionDeclineListing(ext.id, storeId))
                  }}
                  disabled={busy}
                >
                  Not this one
                </button>
              </div>
            </div>
          )}
          {ext.pending_update && (
            <div className="row kb-ext-sub">
              <div className="l">
                <span>Update to {ext.pending_update.version} is pending for approval</span>
                <span>It also asks to:</span>
                {ext.pending_update.abilities.map((ability) => (
                  <span key={ability}>{ability}</span>
                ))}
              </div>
              <button
                type="button"
                className="kb-btn"
                onClick={() => void run(() => commands.extensionApproveUpdate(ext.id))}
                disabled={busy}
              >
                Update
              </button>
            </div>
          )}
          {regular && (
            <div className="row kb-ext-sub">
              <div className="l">
                <span>Run in {regular.name}</span>
                <span>Other workspaces keep their own setting.</span>
              </div>
              <Switch
                checked={!ext.disabled_in.includes(regular.id)}
                onChange={(on) => void run(() => commands.extensionSetWorkspace(ext.id, regular.id, on))}
                label={`Run ${ext.name} in ${regular.name}`}
              />
            </div>
          )}
          <div className="row kb-ext-sub">
            <div className="l">
              <span>Allow in private workspaces</span>
              <span>It can then see what you do in private browsing.</span>
            </div>
            <Switch
              checked={ext.allow_private}
              onChange={(allowed) => void run(() => commands.extensionSetPrivate(ext.id, allowed))}
              label={`Allow ${ext.name} in private workspaces`}
            />
          </div>
          <div className="row kb-ext-sub">
            <div className="l">
              <span>Site access</span>
              <span>
                {ext.site_access === 'all'
                  ? 'Every site it asks for.'
                  : 'Only sites you allow from the extensions menu while on them.'}{' '}
                Open pages follow when they reload.
              </span>
            </div>
            <Segmented
              value={ext.site_access}
              onChange={(access) => void run(() => commands.extensionSetSiteAccess(ext.id, access))}
              options={SITE_ACCESS}
              ariaLabel={`Where ${ext.name} runs`}
              disabled={busy}
            />
          </div>
          {ext.site_access === 'allowed' &&
            ext.allowed_sites.map((host) => (
              <div key={host} className="row kb-ext-sub">
                <div className="l">
                  <span>{host}</span>
                </div>
                <button
                  type="button"
                  className="kb-btn"
                  onClick={() => void run(() => commands.extensionSetSite(ext.id, host, false))}
                  disabled={busy}
                  aria-label={`Stop ${ext.name} on ${host}`}
                >
                  Remove
                </button>
              </div>
            ))}
          {ext.granted.length > 0 && (
            <GrantedRow
              ext={ext}
              busy={busy}
              onTakeBack={() => void run(() => commands.extensionRevokeGranted(ext.id))}
            />
          )}
        </div>
      ))}
      {error && <p className="kb-backup-status is-error">{error}</p>}
      <ConfirmDialog
        open={staged !== null}
        title={staged ? `Add ${staged.review.name} ${staged.review.version}?` : ''}
        message={staged ? <ReviewDetails staged={staged} /> : undefined}
        confirmLabel="Add extension"
        onConfirm={() => staged && install(staged.token)}
        onCancel={() => staged && cancelReview(staged.token)}
      />
      <ConfirmDialog
        open={removing !== null}
        title={removing ? `Remove ${removing.name}?` : ''}
        message="It stops running in every workspace, and open pages reload without it. Adding it again later starts it afresh."
        confirmLabel="Remove"
        danger
        onConfirm={() => removing && remove(removing.id)}
        onCancel={() => setRemoving(null)}
      />
    </section>
  )
}
