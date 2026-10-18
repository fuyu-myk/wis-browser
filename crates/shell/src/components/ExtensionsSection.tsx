import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { commands } from '@/bindings/commands'
import type { InstalledExtension, StagedExtension } from '@/bindings/types'
import { ConfirmDialog } from '@/components/ConfirmDialog'
import { errorMessage } from '@/lib/errors'

interface ExtensionsSectionProps {
  installed: InstalledExtension[]
}

function ReviewDetails({ staged }: { staged: StagedExtension }) {
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

export function ExtensionsSection({ installed }: ExtensionsSectionProps) {
  const { data: supported } = useQuery({
    queryKey: ['extensions-supported'],
    queryFn: () => commands.extensionsSupported(),
    staleTime: Infinity,
  })
  const [staged, setStaged] = useState<StagedExtension | null>(null)
  const [removing, setRemoving] = useState<InstalledExtension | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

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

  return (
    <section id="extensions" className="sec">
      <div className="sec-h">Extensions</div>
      <div className="row">
        <div className="l">
          <span>Add an extension</span>
          <span>
            {supported === false
              ? 'Extensions need macOS 15.4 or later, and are not available on Windows or Linux yet.'
              : 'From an unpacked folder or a .zip. They run in your regular workspaces, in tabs opened from then on.'}
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
      {installed.map((ext) => (
        <div className="row" key={ext.id}>
          <div className="l">
            <span>{ext.name}</span>
            <span>Version {ext.version}</span>
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
        message="It stops running in every workspace. Pages it already changed stay that way until you reload them, and adding it again later starts it afresh."
        confirmLabel="Remove"
        danger
        onConfirm={() => removing && remove(removing.id)}
        onCancel={() => setRemoving(null)}
      />
    </section>
  )
}
