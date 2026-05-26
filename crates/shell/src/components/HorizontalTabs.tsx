import { useLayoutEffect, useRef, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { Tab, TabId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { LockIcon, XIcon } from '@/lib/icons'
import { tabDisplayName } from '@/lib/url'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'

import { TabFavicon } from '@/components/TabFavicon'

interface HorizontalTabsProps {
  tabs: Tab[]
  activeTabId: TabId | null
  /** When true, render the URL inside the active tab. */
  integrateUrlBar: boolean
  /** Render as a smaller, centered tab strip. */
  compact?: boolean
}

/**
 * Horizontal tab strip.
 *
 * Layout rules:
 *   • 1 tab → fills the row.
 *   • n ≤ MAX_EVEN tabs → each takes 1/n of the row.
 *   • n > MAX_EVEN tabs → active tab gets ACTIVE_PRESET_PX, others
 *     share the remaining space evenly down to INACTIVE_MIN_PX.
 *
 * Overflow: once pills can't honour their min width the
 * strip becomes horizontally scrollable with a fade-mask at each edge.
 *
 * When `integrateUrlBar` is true the active tab renders the URL parts
 * instead of the title.
 */
const MAX_EVEN = 5
const ACTIVE_PRESET_PX = 260
const INACTIVE_MIN_PX = 56
const COMPACT_ACTIVE_PRESET_PX = 180
const COMPACT_INACTIVE_MIN_PX = 52

export function HorizontalTabs({
  tabs,
  activeTabId,
  integrateUrlBar,
  compact = false,
}: HorizontalTabsProps) {
  const qc = useQueryClient()
  const setActive = useMutation({
    mutationFn: (id: TabId) => commands.setActiveTab(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })
  const close = useMutation({
    mutationFn: (id: TabId) => commands.closeTab(id),
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })

  const wrapRef = useRef<HTMLDivElement>(null)
  const stripRef = useRef<HTMLDivElement>(null)
  const [scrollable, setScrollable] = useState(false)

  useLayoutEffect(() => {
    const strip = stripRef.current
    if (!strip) return
    const update = () => setScrollable(strip.scrollWidth > strip.clientWidth + 1)
    update()
    const ro = new ResizeObserver(update)
    ro.observe(strip)
    return () => ro.disconnect()
  }, [tabs.length, compact])

  const n = tabs.length
  if (n === 0) {
    return (
      <div className={cx('kb-tabs-h-wrap', { compact })} ref={wrapRef}>
        <div className={cx('tabs-h', { compact })} ref={stripRef} />
      </div>
    )
  }

  const useEven = n <= MAX_EVEN
  const evenPct = useEven ? 100 / n : 0
  const activePresetPx = compact ? COMPACT_ACTIVE_PRESET_PX : ACTIVE_PRESET_PX
  const inactiveMinPx = compact ? COMPACT_INACTIVE_MIN_PX : INACTIVE_MIN_PX

  return (
    <div className={cx('kb-tabs-h-wrap', { compact, 'is-scrollable': scrollable })} ref={wrapRef}>
      <div className={cx('tabs-h', { compact })} ref={stripRef}>
        {tabs.map((tab) => {
          const isActive = tab.id === activeTabId
          const style = useEven
            ? { flex: `1 1 ${evenPct}%`, maxWidth: `${evenPct}%`, minWidth: `${inactiveMinPx}px` }
            : isActive
              ? { flex: `0 0 ${activePresetPx}px` }
              : { flex: `1 1 ${inactiveMinPx}px`, minWidth: `${inactiveMinPx}px` }

          return (
            <div
              key={tab.id}
              role="button"
              tabIndex={0}
              className={cx('h-tab', {
                active: isActive,
                'as-urlbar': isActive && integrateUrlBar,
              })}
              style={style}
              onClick={() => setActive.mutate(tab.id)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault()
                  setActive.mutate(tab.id)
                }
              }}
              title={tab.title}
            >
              {isActive && integrateUrlBar ? (
                <UrlContent tab={tab} />
              ) : (
                <>
                  <TabFavicon tab={tab} />
                  <span className="ttl">{tabDisplayName(tab)}</span>
                </>
              )}
              <button
                type="button"
                className="close"
                aria-label="Close tab"
                onClick={(e) => {
                  e.stopPropagation()
                  close.mutate(tab.id)
                }}
              >
                <XIcon />
              </button>
            </div>
          )
        })}
      </div>
    </div>
  )
}

function UrlContent({ tab }: { tab: Tab }) {
  const parts = (() => {
    try {
      const u = new URL(tab.url)
      if (u.protocol === 'about:' || u.host === '') {
        return null
      }
      return { domain: u.host, path: u.pathname === '/' ? '' : u.pathname }
    } catch {
      return null
    }
  })()

  return (
    <>
      <LockIcon className="ico" />
      {parts ? (
        <>
          <span className="domain">{parts.domain}</span>
          <span className="path">{parts.path}</span>
        </>
      ) : (
        <span className="path">Search or enter URL</span>
      )}
    </>
  )
}

