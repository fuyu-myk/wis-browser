/**
 * Taking an extension's updates from the Chrome Web Store page it is on, from
 * the extensions menu: the listing's package is checked to be the same
 * extension before the two are tied.
 */

import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { emit } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import { STORE_PANEL, type PanelSize } from '@/features/extensions/hooks/useExtensionsMenu'
import { cx } from '@/lib/cx'
import { errorMessage } from '@/lib/errors'
import { EXT_PROMPT_SIZE } from '@/features/extensions/lib/extensionsMenu'
import { describeLinked } from '@/features/extensions/lib/extensionUpdates'

/** The panel's top and bottom border. */
const BORDERS = 2

type Step = { kind: 'linking' } | { kind: 'done'; message: string } | { kind: 'failed'; message: string }

interface ExtensionStoreLinkProps {
  id: string
  storeId: string
  name: string
  glass: boolean
  onDone: () => void
}

export function ExtensionStoreLink({ id, storeId, name, glass, onDone }: ExtensionStoreLinkProps) {
  const [step, setStep] = useState<Step>({ kind: 'linking' })
  const bodyRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    let live = true
    commands.extensionLinkStore(id, storeId).then(
      (linked) => live && setStep({ kind: 'done', message: describeLinked(name, linked) }),
      (err: unknown) => live && setStep({ kind: 'failed', message: errorMessage(err) }),
    )
    return () => {
      live = false
    }
  }, [id, storeId, name])

  useLayoutEffect(() => {
    const body = bodyRef.current
    if (!body) return
    const size: PanelSize = {
      key: STORE_PANEL,
      height: Math.ceil(body.getBoundingClientRect().height) + BORDERS,
    }
    void emit(EXT_PROMPT_SIZE, size)
  }, [step])

  return (
    <div
      className={cx('kb-extmenu kb-extprompt', { 'glass-menus': glass })}
      role="alertdialog"
      aria-label={`Updates for ${name}`}
      onKeyDown={(e) => {
        if (e.key === 'Escape') onDone()
      }}
    >
      <div ref={bodyRef} className="kb-extprompt-body">
        {step.kind === 'linking' && <span className="kb-extprompt-note">Checking this listing’s {name}…</span>}
        {step.kind === 'failed' && (
          <span className="kb-extprompt-title">{name} was not linked to this listing</span>
        )}
        {step.kind !== 'linking' && (
          <>
            <span className="kb-extprompt-note">{step.message}</span>
            <div className="kb-perm-actions">
              <button type="button" className="kb-perm-btn" onClick={onDone} autoFocus>
                Close
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  )
}
