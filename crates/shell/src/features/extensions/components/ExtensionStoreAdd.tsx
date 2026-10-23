/**
 * Adding a Chrome Web Store page's extension from the extensions menu: its
 * package is fetched and checked, then reviewed like any other.
 */

import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { emit } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { StagedExtension } from '@/bindings/types'
import { ReviewDetails } from '@/features/extensions/components/ExtensionsSection'
import { STORE_PANEL, type PanelSize } from '@/features/extensions/hooks/useExtensionsMenu'
import { cx } from '@/lib/cx'
import { errorMessage } from '@/lib/errors'
import { EXT_PROMPT_SIZE } from '@/features/extensions/lib/extensionsMenu'

/** The panel's top and bottom border. */
const BORDERS = 2

type Step =
  | { kind: 'getting' }
  | { kind: 'review'; staged: StagedExtension; adding: boolean }
  | { kind: 'failed'; message: string }

interface ExtensionStoreAddProps {
  storeId: string
  glass: boolean
  stagedRef: React.MutableRefObject<string | null>
  onDone: () => void
}

export function ExtensionStoreAdd({ storeId, glass, stagedRef, onDone }: ExtensionStoreAddProps) {
  const [step, setStep] = useState<Step>({ kind: 'getting' })
  const bodyRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    let live = true
    commands.extensionStageFromStore(storeId).then(
      (staged) => {
        if (!live) {
          void commands.extensionDiscard(staged.token).catch(() => undefined)
          return
        }
        stagedRef.current = staged.token
        setStep({ kind: 'review', staged, adding: false })
      },
      (err: unknown) => {
        if (live) setStep({ kind: 'failed', message: errorMessage(err) })
      },
    )
    return () => {
      live = false
    }
  }, [storeId, stagedRef])

  useLayoutEffect(() => {
    const body = bodyRef.current
    if (!body) return
    const size: PanelSize = {
      key: STORE_PANEL,
      height: Math.ceil(body.getBoundingClientRect().height) + BORDERS,
    }
    void emit(EXT_PROMPT_SIZE, size)
  }, [step])

  const cancel = () => {
    if (stagedRef.current) void commands.extensionDiscard(stagedRef.current).catch(() => undefined)
    stagedRef.current = null
    onDone()
  }

  const add = (staged: StagedExtension) => {
    setStep({ kind: 'review', staged, adding: true })
    stagedRef.current = null
    commands.extensionInstall(staged.token).then(onDone, (err: unknown) =>
      setStep({ kind: 'failed', message: errorMessage(err) }),
    )
  }

  return (
    <div
      className={cx('kb-extmenu kb-extprompt', { 'glass-menus': glass })}
      role="alertdialog"
      aria-label="Add from the Chrome Web Store"
      onKeyDown={(e) => {
        if (e.key === 'Escape') cancel()
      }}
    >
      <div ref={bodyRef} className="kb-extprompt-body">
        {step.kind === 'getting' && <span className="kb-extprompt-note">Getting the extension…</span>}
        {step.kind === 'failed' && (
          <>
            <span className="kb-extprompt-title">The extension could not be added</span>
            <span className="kb-extprompt-note">{step.message}</span>
            <div className="kb-perm-actions">
              <button type="button" className="kb-perm-btn" onClick={cancel} autoFocus>
                Close
              </button>
            </div>
          </>
        )}
        {step.kind === 'review' && (
          <>
            <span className="kb-extprompt-title">
              Add {step.staged.review.name} {step.staged.review.version}?
            </span>
            <div className="kb-extprompt-review">
              <ReviewDetails staged={step.staged} />
            </div>
            <div className="kb-perm-actions">
              <button type="button" className="kb-perm-btn" onClick={cancel} disabled={step.adding}>
                Cancel
              </button>
              <button
                type="button"
                className="kb-perm-btn is-primary"
                onClick={() => add(step.staged)}
                disabled={step.adding}
                autoFocus
              >
                {step.adding ? 'Adding…' : 'Add extension'}
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  )
}
