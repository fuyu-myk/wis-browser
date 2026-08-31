// Curated catalog of built-in sidebar widgets (social apps, AI providers,
// productivity sites) plus user-defined custom-URL widgets. The enum of kinds
// lives in Rust (`wis_core::widget::WidgetKind`); the labels, URLs, and
// grouping are presentation data and live here so adding a site is a one-line
// change with no IPC.

import type { Widget, WidgetGroup, WidgetId, WidgetKind } from '@/bindings/types'

export type WidgetCategory = WidgetGroup

/** Canonical dock row order; `sidebar_widgets` stores rows in this grouping. */
export const WIDGET_GROUPS: ReadonlyArray<WidgetCategory> = [
  'social',
  'ai',
  'productivity',
  'custom',
]

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
  { kind: 'perplexity', label: 'Perplexity', url: 'https://www.perplexity.ai', category: 'ai' },
  // Productivity.
  { kind: 'gmail', label: 'Gmail', url: 'https://mail.google.com', category: 'productivity' },
  {
    kind: 'google_calendar',
    label: 'Google Calendar',
    url: 'https://calendar.google.com',
    category: 'productivity',
  },
  {
    kind: 'google_keep',
    label: 'Google Keep',
    url: 'https://keep.google.com',
    category: 'productivity',
  },
  { kind: 'notion', label: 'Notion', url: 'https://www.notion.so', category: 'productivity' },
  { kind: 'todoist', label: 'Todoist', url: 'https://app.todoist.com', category: 'productivity' },
  {
    kind: 'outlook',
    label: 'Outlook',
    url: 'https://outlook.live.com/mail',
    category: 'productivity',
  },
  {
    kind: 'proton_mail',
    label: 'Proton Mail',
    url: 'https://mail.proton.me',
    category: 'productivity',
  },
  {
    kind: 'proton_calendar',
    label: 'Proton Calendar',
    url: 'https://calendar.proton.me',
    category: 'productivity',
  },
]

const CATALOG_INDEX = new Map(WIDGET_CATALOG.map((entry) => [entry.kind, entry]))

/** Catalog metadata for a kind (undefined for `custom`). */
export function catalogEntry(kind: WidgetKind): WidgetCatalogEntry | undefined {
  return CATALOG_INDEX.get(kind as WidgetCatalogEntry['kind'])
}

export function widgetCategory(widget: Widget): WidgetCategory {
  if (widget.kind === 'custom') return widget.group ?? 'custom'
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
    group: null,
  }
}

/** Cap on a custom widget's label, mirroring `MAX_WIDGET_LABEL_CHARS` in core. */
export const MAX_CUSTOM_WIDGET_LABEL_CHARS = 48

/** Cap on a custom widget's URL, mirroring `MAX_WIDGET_URL_BYTES` in core. */
export const MAX_CUSTOM_WIDGET_URL_BYTES = 2048

/** A bare domain gets https; anything already carrying a scheme is left alone. */
export function normalizeCustomWidgetUrl(raw: string): string {
  const value = raw.trim()
  if (value === '' || value.includes('://')) return value
  return `https://${value}`
}

/**
 * Why `raw` cannot be a custom widget URL, or null when it can. Mirrors
 * `Widget::validate` in core.
 */
export function customWidgetUrlProblem(raw: string): string | null {
  const value = normalizeCustomWidgetUrl(raw)
  if (value === '') return null

  let parsed: URL
  try {
    parsed = new URL(value)
  } catch {
    return 'Enter a web address, like notion.so or https://example.com.'
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
    return 'Only http:// and https:// sites can be widgets.'
  }
  if (parsed.hostname === '') {
    return 'Enter a web address, like notion.so or https://example.com.'
  }
  if (new TextEncoder().encode(value).length > MAX_CUSTOM_WIDGET_URL_BYTES) {
    return 'That web address is too long for a widget.'
  }

  return null
}

/**
 * The stored vec with widget `id` placed in row `target` at `index`.
 * Returns null when the move is not allowed as only custom widgets
 * may change rows. A custom landing on another row stores it as its
 * group.
 */
export function placeWidget(
  widgets: readonly Widget[],
  id: WidgetId,
  target: WidgetCategory,
  index: number,
): Widget[] | null {
  const dragged = widgets.find((w) => w.id === id)
  if (dragged === undefined) return null
  if (widgetCategory(dragged) !== target && dragged.kind !== 'custom') return null

  const moved: Widget =
    dragged.kind === 'custom'
      ? { ...dragged, group: target === 'custom' ? null : target }
      : dragged
  return WIDGET_GROUPS.flatMap((category) => {
    const row = widgets.filter((w) => widgetCategory(w) === category && w.id !== id)
    if (category !== target) return row
    const clamped = Math.min(Math.max(index, 0), row.length)
    return [...row.slice(0, clamped), moved, ...row.slice(clamped)]
  })
}

/** What the dock actually shows: each row's id order, in canonical row order. */
export function dockSignature(widgets: readonly Widget[]): string {
  return WIDGET_GROUPS.map((category) =>
    widgets
      .filter((w) => widgetCategory(w) === category)
      .map((w) => w.id)
      .join(','),
  ).join('|')
}

/** A fresh enabled custom widget for a user-supplied site, placed in `group`. */
export function makeCustomWidget(label: string, url: string, group: WidgetCategory): Widget {
  return {
    id: crypto.randomUUID() as WidgetId,
    kind: 'custom',
    label: label.trim(),
    url: normalizeCustomWidgetUrl(url),
    icon: null,
    enabled: true,
    group: group === 'custom' ? null : group,
  }
}
