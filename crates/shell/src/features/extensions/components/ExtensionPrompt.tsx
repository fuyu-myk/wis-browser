/**
 * A running extension asking for more access, shown in the extensions menu's
 * overlay under the puzzle button. The answer covers the whole request.
 */

import { useLayoutEffect, useRef } from 'react'
import { emit } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { ExtensionPermissionRequest } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { EXT_PROMPT_SIZE } from '@/features/extensions/lib/extensionsMenu'

/** The panel's top and bottom border. */
const BORDERS = 2

interface ExtensionPromptProps {
  request: ExtensionPermissionRequest
  glass: boolean
  onDone: () => void
}

export function ExtensionPrompt({ request, glass, onDone }: ExtensionPromptProps) {
  const bodyRef = useRef<HTMLDivElement>(null)

  useLayoutEffect(() => {
    const body = bodyRef.current
    if (!body) return
    void emit(EXT_PROMPT_SIZE, {
      key: `prompt-${request.request}`,
      height: Math.ceil(body.getBoundingClientRect().height) + BORDERS,
    })
  }, [request])

  const decide = (allow: boolean) => {
    void commands.extensionAnswerPermission(request.request, allow).catch(() => undefined)
    onDone()
  }

  return (
    <div
      className={cx('kb-extmenu kb-extprompt', { 'glass-menus': glass })}
      role="alertdialog"
      aria-label={`${request.name} wants more access`}
      onKeyDown={(e) => {
        if (e.key === 'Escape') decide(false)
      }}
    >
      <div ref={bodyRef} className="kb-extprompt-body">
        <span className="kb-extprompt-title">{request.name} wants to:</span>
        <ul className="kb-extprompt-list">
          {request.abilities.map((ability) => (
            <li key={ability}>{ability}</li>
          ))}
        </ul>
        <span className="kb-extprompt-note">You can take this back in Settings, under Extensions.</span>
        <div className="kb-perm-actions">
          <button type="button" className="kb-perm-btn" onClick={() => decide(false)} autoFocus>
            Don’t allow
          </button>
          <button type="button" className="kb-perm-btn is-primary" onClick={() => decide(true)}>
            Allow
          </button>
        </div>
      </div>
    </div>
  )
}
