// Curated catalog of built-in sidebar widgets (social apps + AI providers).
// The enum of kinds lives in Rust (`wis_core::widget::WidgetKind`); the labels,
// URLs, and grouping are presentation data and live here so adding a site is a
// one-line change with no IPC.

import type { Widget, WidgetId, WidgetKind } from '@/bindings/types'

export type WidgetCategory = 'social' | 'ai'

/**
 * Default cap on live widget-panel webviews. Each is a full page (a heavy
 * SPA can hold 100-400 MB), so the pool stays small by default: beyond the
 * cap the least-recently-used hidden panel is destroyed. User-tunable via
 * `widget_panel_limit_enabled` / `widget_panel_limit` in settings.
 */
export const DEFAULT_PANEL_LIMIT = 3

export interface WidgetCatalogEntry {
  kind: Exclude<WidgetKind, 'custom'>
  label: string
  url: string
  category: WidgetCategory
}

export const WIDGET_CATALOG: ReadonlyArray<WidgetCatalogEntry> = [
  // Social / messaging.
  { kind: 'whats_app', label: 'WhatsApp', url: 'https://web.whatsapp.com', category: 'social' },
  { kind: 'telegram', label: 'Telegram', url: 'https://web.telegram.org/a/', category: 'social' },
  { kind: 'messenger', label: 'Messenger', url: 'https://www.messenger.com', category: 'social' },
  { kind: 'line', label: 'LINE', url: 'https://line.me', category: 'social' },
  { kind: 'discord', label: 'Discord', url: 'https://discord.com/app', category: 'social' },
  { kind: 'slack', label: 'Slack', url: 'https://app.slack.com', category: 'social' },
  { kind: 'x_twitter', label: 'X', url: 'https://x.com', category: 'social' },
  { kind: 'instagram', label: 'Instagram', url: 'https://www.instagram.com', category: 'social' },
  { kind: 'reddit', label: 'Reddit', url: 'https://www.reddit.com', category: 'social' },
  // AI providers.
  { kind: 'chat_gpt', label: 'ChatGPT', url: 'https://chatgpt.com', category: 'ai' },
  { kind: 'claude', label: 'Claude', url: 'https://claude.ai', category: 'ai' },
  { kind: 'gemini', label: 'Gemini', url: 'https://gemini.google.com', category: 'ai' },
  { kind: 'grok', label: 'Grok', url: 'https://grok.com', category: 'ai' },
  { kind: 'deep_seek', label: 'DeepSeek', url: 'https://chat.deepseek.com', category: 'ai' },
  { kind: 'qwen', label: 'Qwen', url: 'https://chat.qwen.ai', category: 'ai' },
]

const CATALOG_INDEX = new Map(WIDGET_CATALOG.map((entry) => [entry.kind, entry]))

/** Catalog metadata for a kind (undefined for `custom`). */
export function catalogEntry(kind: WidgetKind): WidgetCatalogEntry | undefined {
  return CATALOG_INDEX.get(kind as WidgetCatalogEntry['kind'])
}

export function widgetCategory(widget: Widget): WidgetCategory {
  return catalogEntry(widget.kind)?.category ?? 'social'
}

/**
 * The URL a widget's panel should load given the current chrome theme.
 * Instagram opts into its web dark mode via a `?theme=dark` query parameter
 * rather than `prefers-color-scheme`, so the chrome theme is mirrored onto
 * the URL; every other widget loads its catalog URL unchanged.
 */
export function effectiveWidgetUrl(widget: Widget, dark: boolean): string {
  if (widget.kind === 'telegram') {
    try {
      const u = new URL(widget.url)
      const legacyPath = u.pathname === '/' || u.pathname.startsWith('/k')
      if (u.hostname === 'web.telegram.org' && legacyPath) {
        return 'https://web.telegram.org/a/'
      }
    } catch {
      return widget.url
    }
    return widget.url
  }
  if (widget.kind !== 'instagram') return widget.url
  try {
    const u = new URL(widget.url)
    if (dark) u.searchParams.set('theme', 'dark')
    else u.searchParams.delete('theme')
    return u.toString()
  } catch {
    return widget.url
  }
}

/** A fresh enabled `Widget` record from a catalog entry. */
export function makeWidget(entry: WidgetCatalogEntry): Widget {
  return {
    id: crypto.randomUUID() as WidgetId,
    kind: entry.kind,
    label: entry.label,
    url: entry.url,
    icon: null,
    enabled: true,
  }
}
