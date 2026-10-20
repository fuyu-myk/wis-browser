import type { AutoplayPolicy, UaProfile, UserSettings, UserSettingsPatch } from '@/bindings/types'
import { normalizeHostInput } from '@/lib/host'

export type SiteRuleSettings = Pick<
  UserSettings,
  | 'media_permissions'
  | 'autoplay_site_overrides'
  | 'zoom_site_overrides'
  | 'ua_site_overrides'
  | 'app_launch_permissions'
  | 'user_cosmetic_rules'
>

export interface SiteRule {
  id: string
  label: string
  value: string
}

export const AUTOPLAY_OPTIONS = [
  { value: 'allow' as const, label: 'Allow' },
  { value: 'block_audible' as const, label: 'Block sound' },
  { value: 'block' as const, label: 'Block all' },
] satisfies ReadonlyArray<{ value: AutoplayPolicy; label: string }>

export function autoplayLabel(policy: AutoplayPolicy): string {
  return AUTOPLAY_OPTIONS.find((o) => o.value === policy)?.label ?? policy
}

export function uaProfileLabel(profile: UaProfile): string {
  if (profile === 'safari') return 'Safari'
  if (profile === 'chrome') return 'Chrome'
  if (profile === 'default') return 'Engine default'
  return 'Custom'
}

export function deviceLabel(kind: string): string {
  if (kind === 'microphone') return 'Microphone'
  if (kind === 'camera+microphone') return 'Camera & mic'
  return 'Camera'
}

/** Splits a `"<first>|<rest>"` permission key. */
function splitKey(key: string): [string, string] {
  const sep = key.indexOf('|')
  return [key.slice(0, sep), key.slice(sep + 1)]
}

function onSite(host: string, site: string): boolean {
  return normalizeHostInput(host) === site
}

function entriesOn<T>(map: Record<string, T>, site: string): [string, T][] {
  return Object.entries(map).filter(([host]) => onSite(host, site))
}

function without<T>(map: Record<string, T>, drop: (key: string) => boolean): Record<string, T> {
  return Object.fromEntries(Object.entries(map).filter(([key]) => !drop(key)))
}

const decision = (allowed: boolean) => (allowed ? 'Allowed' : 'Blocked')

/** One row per `"<first>|<host>"` key on the site. */
function keyedRules(
  map: Record<string, boolean>,
  site: string,
  prefix: string,
  label: (first: string) => string,
): SiteRule[] {
  return Object.entries(map)
    .filter(([key]) => onSite(splitKey(key)[1], site))
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([key, allowed]) => ({ id: `${prefix}|${key}`, label: label(splitKey(key)[0]), value: decision(allowed) }))
}

function hostRule<T>(map: Record<string, T>, site: string, id: string, label: string, value: (v: T) => string): SiteRule[] {
  const [entry] = entriesOn(map, site)
  return entry ? [{ id, label, value: value(entry[1]) }] : []
}

export function siteRules(settings: SiteRuleSettings, site: string): SiteRule[] {
  const hidden = entriesOn(settings.user_cosmetic_rules, site).reduce((n, [, selectors]) => n + selectors.length, 0)
  return [
    ...keyedRules(settings.media_permissions, site, 'media', deviceLabel),
    ...hostRule(settings.autoplay_site_overrides, site, 'autoplay', 'Autoplay', autoplayLabel),
    ...hostRule(settings.zoom_site_overrides, site, 'zoom', 'Zoom', (percent) => `${percent}%`),
    ...hostRule(settings.ua_site_overrides, site, 'ua', 'User agent', uaProfileLabel),
    ...keyedRules(settings.app_launch_permissions, site, 'app_launch', (scheme) => `${scheme} links`),
    ...(hidden > 0 ? [{ id: 'hidden', label: 'Hidden elements', value: String(hidden) }] : []),
  ]
}

export function forgetSiteRule(settings: SiteRuleSettings, site: string, id: string): UserSettingsPatch {
  if (id.startsWith('media|')) {
    const key = id.slice('media|'.length)
    return { media_permissions: without(settings.media_permissions, (k) => k === key) }
  }
  if (id.startsWith('app_launch|')) {
    const key = id.slice('app_launch|'.length)
    return { app_launch_permissions: without(settings.app_launch_permissions, (k) => k === key) }
  }
  const forHost = (host: string) => onSite(host, site)
  switch (id) {
    case 'autoplay':
      return { autoplay_site_overrides: without(settings.autoplay_site_overrides, forHost) }
    case 'zoom':
      return { zoom_site_overrides: without(settings.zoom_site_overrides, forHost) }
    case 'ua':
      return { ua_site_overrides: without(settings.ua_site_overrides, forHost) }
    case 'hidden':
      return { user_cosmetic_rules: without(settings.user_cosmetic_rules, forHost) }
    default:
      return {}
  }
}
