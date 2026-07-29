import { useState } from 'react'

import { useMutation } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { Tab } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { ArrowLeftIcon, ArrowRightIcon, ReaderIcon, RefreshIcon } from '@/lib/icons'
import { isChromeUrl } from '@/lib/url'

interface NavRowProps {
  /** Active tab the buttons act on. When `null`, the row is fully disabled. */
  activeTab: Tab | null
  /** Render the buttons as standalone glass circles. */
  circular?: boolean
}

/**
 * Back / Forward / Refresh strip.
 *
 * wry 0.55 doesn't expose native `go_back` / `go_forward` methods, so
 * fire `eval("window.history.back()")` via the `tab_go_back/forward`
 * commands. Back/Forward disable per direction from the tab's
 * `can_go_back`/`can_go_forward` (engine-read on load and title change).
 *
 * The Refresh button calls wry's native `webview.reload()`.
 */
export function NavRow({ activeTab, circular = false }: NavRowProps) {
  const btnClass = circular ? 'kb-circle-btn' : 'kb-iconbtn'
  // Bumped per press; keying the icon wrapper replays the spin each time.
  const [spinNonce, setSpinNonce] = useState(0)

  const back = useMutation({
    mutationFn: (id: Tab['id']) => commands.tabGoBack(id),
  })
  const forward = useMutation({
    mutationFn: (id: Tab['id']) => commands.tabGoForward(id),
  })
  const reload = useMutation({
    mutationFn: (id: Tab['id']) => commands.tabReload(id),
  })
  const reader = useMutation({
    mutationFn: ({ id, enabled }: { id: Tab['id']; enabled: boolean }) =>
      commands.tabSetReader(id, enabled),
  })

  // No active tab: or active tab is a chrome page with no
  // history of its own.
  const navigable = activeTab !== null && !isChromeUrl(activeTab.url)

  return (
    <div className={cx('kb-nav-row', { 'is-circular': circular })}>
      <button
        type="button"
        className={btnClass}
        aria-label="Back"
        disabled={!navigable || !activeTab.can_go_back}
        onClick={() => activeTab && back.mutate(activeTab.id)}
      >
        <ArrowLeftIcon />
      </button>
      <button
        type="button"
        className={btnClass}
        aria-label="Forward"
        disabled={!navigable || !activeTab.can_go_forward}
        onClick={() => activeTab && forward.mutate(activeTab.id)}
      >
        <ArrowRightIcon />
      </button>
      <button
        type="button"
        className={btnClass}
        aria-label="Refresh"
        disabled={!navigable}
        onClick={() => {
          if (!activeTab) return
          reload.mutate(activeTab.id)
          setSpinNonce((n) => n + 1)
        }}
      >
        <span
          key={spinNonce}
          className={cx('kb-reload-spin', { 'is-spinning': spinNonce > 0 })}
          aria-hidden
        >
          <RefreshIcon />
        </span>
      </button>
      <button
        type="button"
        className={cx(btnClass, { 'is-active': activeTab?.reader })}
        aria-label="Reader mode"
        aria-pressed={activeTab?.reader ?? false}
        disabled={!navigable}
        onClick={() => {
          if (!activeTab) return
          reader.mutate({ id: activeTab.id, enabled: !activeTab.reader })
        }}
      >
        <ReaderIcon />
      </button>
    </div>
  )
}
