/**
 * Settings -> Performance: live per-webview memory readout (each webview's
 * WebContent process footprint, the figure Activity Monitor shows), with
 * one-click reclaim for tabs and widget panels. Polls while mounted; the
 * settings page only renders while its tab is shown, so the poll stops when
 * the page goes away.
 */

import { useQuery, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { BrowserState, TabId, WebviewMemory } from '@/bindings/types'
import { closeTabAnimated } from '@/lib/tabClose'
import { XIcon } from '@/lib/icons'
import { isMacOS } from '@/lib/platform'
import { tabDisplayName } from '@/lib/url'
import { BROWSER_STATE_KEY, useBrowserState } from '@/hooks/useBrowserState'

const POLL_MS = 3000
const TAB_PREFIX = 'tab-'
const WIDGET_PREFIX = 'widget-panel-'

interface RowInfo {
  name: string
  kind: string
  close?: () => void
}

function rowInfo(
  row: WebviewMemory,
  state: BrowserState | undefined,
  invalidate: () => void,
): RowInfo {
  const { label } = row
  if (label === 'main') return { name: 'Wis chrome', kind: 'Chrome' }
  if (label.startsWith('peek-page')) return { name: 'Peek preview', kind: 'Peek' }
  if (label.startsWith('overlay-')) {
    return { name: `Overlay (${label.slice('overlay-'.length)})`, kind: 'Chrome' }
  }
  if (label.startsWith(WIDGET_PREFIX)) {
    // Labels are per-window (`widget-panel-<window>-<id>`); match the widget by
    // its id as the label suffix.
    const widget = state?.settings.sidebar_widgets.find((w) => label.endsWith(`-${w.id}`))
    const id = widget?.id
    return {
      name: widget?.label ?? 'Widget',
      kind: 'Widget',
      close: id ? () => void commands.closeWidgetPanel(id).catch(() => undefined) : undefined,
    }
  }
  if (label.startsWith(TAB_PREFIX)) {
    const id = label.slice(TAB_PREFIX.length) as TabId
    const tab = state?.tabs.find((t) => t.id === id)
    return {
      name: tab ? tabDisplayName(tab) : 'Tab',
      kind: 'Tab',
      close: () => {
        closeTabAnimated(id)
        invalidate()
      },
    }
  }
  return { name: label, kind: 'Other' }
}

function fmtBytes(n: number): string {
  if (n < 1024 * 1024) return `${Math.round(n / 1024)} KB`
  const mb = n / (1024 * 1024)
  if (mb < 1024) return `${Math.round(mb)} MB`
  return `${(mb / 1024).toFixed(2)} GB`
}

export function TaskManagerSection() {
  const { data: state } = useBrowserState()
  const qc = useQueryClient()
  const invalidate = () => void qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY })

  const { data: rows = [] } = useQuery({
    queryKey: ['webview-memory'],
    queryFn: () => commands.listWebviewMemory(),
    refetchInterval: POLL_MS,
    // Per-tab memory is macOS only (see memory.rs); skip the poll elsewhere.
    enabled: isMacOS(),
  })

  const sorted = [...rows].sort((a, b) => b.bytes - a.bytes)
  const total = rows.reduce((sum, r) => sum + r.bytes, 0)

  if (!isMacOS()) {
    return (
      <div className="row">
        <div className="l">
          <span>Memory by webview</span>
          <span>
            Per-tab memory is available on macOS only: WebView2 shares one
            process across tabs and exposes no per-tab figure.
          </span>
        </div>
      </div>
    )
  }

  return (
    <>
      <div className="row">
        <div className="l">
          <span>Memory by webview</span>
          <span>
            Each page process's physical footprint, as reported by the OS;
            refreshed every few seconds. Total: {fmtBytes(total)}.
          </span>
        </div>
      </div>
      <ul className="kb-taskman-list">
        {sorted.map((r) => {
          const info = rowInfo(r, state, invalidate)
          return (
            <li className="kb-taskman-row" key={r.label}>
              <span className="kb-taskman-kind">{info.kind}</span>
              <span className="kb-taskman-name" title={r.label}>
                {info.name}
              </span>
              <span className="kb-taskman-bytes">{fmtBytes(r.bytes)}</span>
              {info.close !== undefined ? (
                <button
                  type="button"
                  className="kb-taskman-x"
                  aria-label={`Close ${info.name}`}
                  title="Close"
                  onClick={info.close}
                >
                  <XIcon />
                </button>
              ) : (
                <span className="kb-taskman-noaction" aria-hidden />
              )}
            </li>
          )
        })}
        {sorted.length === 0 && <li className="kb-taskman-empty">No webviews measured.</li>}
      </ul>
    </>
  )
}
