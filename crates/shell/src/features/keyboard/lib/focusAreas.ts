import { commands } from '@/bindings/commands'
import type { Tab } from '@/bindings/types'
import { firstStop } from '@/lib/tabFocus'
import { isChromeUrl } from '@/lib/url'

/** The parts of a window F6 cycles through, in order. */
export type FocusArea = 'url' | 'tabs' | 'page'

const ORDER: readonly FocusArea[] = ['url', 'tabs', 'page']

/** The area after `current` (`other` is somewhere else in the chrome). */
export function nextArea(current: FocusArea | 'other', backwards: boolean): FocusArea {
  if (current === 'other') return backwards ? 'page' : 'url'
  const at = ORDER.indexOf(current)
  return ORDER[(at + (backwards ? ORDER.length - 1 : 1)) % ORDER.length]
}

/** Where keyboard focus is now; the page when the chrome webview lacks it. */
export function currentArea(): FocusArea | 'other' {
  if (!document.hasFocus()) return 'page'
  const active = document.activeElement
  if (active?.closest('.kb-urlpill-inline, .kb-urlbar-input')) return 'url'
  if (active?.closest('[data-tab-strip]')) return 'tabs'
  if (active?.closest('.kb-content')) return 'page'
  return 'other'
}

function focusTabs(): void {
  const strip = document.querySelector<HTMLElement>('[data-tab-strip]')
  const row =
    document.querySelector<HTMLElement>('[data-tab-strip] [data-tab-row][tabindex="0"]') ??
    (strip ? firstStop(strip) : null)
  row?.focus()
}

/**
 * Move keyboard focus to `area`. The page is the active tab's webview, or the
 * chrome's own page (Settings, History) for tabs without one.
 */
export async function focusArea(area: FocusArea, activeTab: Tab | null, focusUrlBar: () => void): Promise<void> {
  if (area === 'page' && activeTab && !isChromeUrl(activeTab.url)) {
    await commands.focusWebview(`tab-${activeTab.id}`).catch(() => undefined)
    return
  }
  await commands.focusSelf().catch(() => undefined)
  if (area === 'url') focusUrlBar()
  else if (area === 'tabs') focusTabs()
  else {
    const content = document.querySelector<HTMLElement>('.kb-content')
    ;(content ? firstStop(content) : null)?.focus()
  }
}
