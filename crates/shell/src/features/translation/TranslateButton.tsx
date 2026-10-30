import { useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { commands } from '@/bindings/commands'
import type { Tab } from '@/bindings/types'
import { translateButton } from '@/features/translation/translation'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'
import { announce } from '@/lib/announce'
import { cx } from '@/lib/cx'
import { errorMessage } from '@/lib/errors'
import { showToast } from '@/lib/toast'

/** Translates the page on-device, or shows its original again. */
export function TranslateButton({ activeTab }: { activeTab: Tab | null }) {
  const qc = useQueryClient()
  const [busy, setBusy] = useState(false)
  const view = translateButton(activeTab)
  if (!activeTab || !view) return null

  const onClick = async () => {
    if (busy) return
    setBusy(true)
    try {
      await commands.tabSetTranslated(activeTab.id, !activeTab.translated)
      announce(activeTab.translated ? 'Showing the original page' : 'Translating the page')
    } catch (err) {
      showToast(errorMessage(err))
    } finally {
      setBusy(false)
      void qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY })
    }
  }

  return (
    <button
      type="button"
      className={cx('kb-urlbar-key', 'kb-urlbar-translate', { 'is-on': view.pressed })}
      title={view.label}
      aria-label={view.label}
      aria-disabled={busy}
      aria-busy={busy}
      onMouseDown={(e) => {
        e.preventDefault()
        e.stopPropagation()
      }}
      onClick={() => void onClick()}
    >
      <TranslateGlyph />
    </button>
  )
}

function TranslateGlyph() {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="m5 8 6 6" />
      <path d="m4 14 6-6 2-3" />
      <path d="M2 5h12" />
      <path d="M7 2h1" />
      <path d="m22 22-5-10-5 10" />
      <path d="M14 18h6" />
    </svg>
  )
}
