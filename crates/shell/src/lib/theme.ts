/**
 * Pure helpers for custom themes: token -> CSS-var mapping (including the
 * primary scale derived from the single accent colour) and the built-in
 * default token sets used to seed a new theme.
 */

import type { ModeTokens } from '@/bindings/types'

function channel(hex: string, i: number): number {
  return parseInt(hex.slice(1 + i * 2, 3 + i * 2), 16)
}

/** Mix `a` toward `b` by `t` (0..1), returning #rrggbb. */
export function mixHex(a: string, b: string, t: number): string {
  const to = (i: number) =>
    Math.round(channel(a, i) + (channel(b, i) - channel(a, i)) * t)
      .toString(16)
      .padStart(2, '0')
  return `#${to(0)}${to(1)}${to(2)}`
}

/** tokens.css's own primary scale for the default accent. */
const DEFAULT_SCALE: Record<string, string> = {
  '--primary': '#8f99fb',
  '--primary-50': '#f0f1ff',
  '--primary-100': '#e2e4fe',
  '--primary-200': '#c5cafd',
  '--primary-300': '#a8b0fc',
  '--primary-400': '#8f99fb',
  '--primary-500': '#6b77f8',
  '--primary-600': '#4a58f4',
  '--primary-700': '#2e3ce0',
  '--primary-800': '#2230b3',
  '--primary-900': '#18237f',
}

/** tokens.css's hand-tuned subtle backgrounds. */
const DEFAULT_BG_SUBTLE = { light: '#f0efeb', dark: '#222229' }

const sameColour = (a: string, b: string) => a.slice(0, 7).toLowerCase() === b.toLowerCase()

/** The primary scale for an accent: lighter mixes toward white above 500,
 * darker toward black below.
 */
function primaryScale(accent: string): Record<string, string> {
  if (sameColour(accent, DEFAULT_SCALE['--primary-500'])) return DEFAULT_SCALE
  const lighter = (t: number) => mixHex(accent, '#ffffff', t)
  const darker = (t: number) => mixHex(accent, '#000000', t)
  return {
    '--primary': lighter(0.24),
    '--primary-50': lighter(0.92),
    '--primary-100': lighter(0.84),
    '--primary-200': lighter(0.66),
    '--primary-300': lighter(0.46),
    '--primary-400': lighter(0.24),
    '--primary-500': accent,
    '--primary-600': darker(0.18),
    '--primary-700': darker(0.36),
    '--primary-800': darker(0.52),
    '--primary-900': darker(0.66),
  }
}

/** CSS variables for one mode's tokens. */
export function themeVars(m: ModeTokens, dark = false): Record<string, string> {
  const seed = defaultModeTokens(dark)
  const atDefaultBg = sameColour(m.bg, seed.bg) && sameColour(m.text_primary, seed.text_primary)
  return {
    ...primaryScale(m.accent.slice(0, 7)),
    '--bg': m.bg,
    '--bg-subtle': atDefaultBg
      ? DEFAULT_BG_SUBTLE[dark ? 'dark' : 'light']
      : mixHex(m.bg.slice(0, 7), m.text_primary.slice(0, 7), 0.04),
    '--surface': m.surface,
    '--surface-raised': m.surface_raised,
    '--chrome-bg': m.chrome_bg,
    '--popover': dark ? mixHex(m.chrome_bg.slice(0, 7), m.bg.slice(0, 7), 0.6) : m.chrome_bg,
    '--content-bg': m.content_bg,
    '--text-primary': m.text_primary,
    '--text-secondary': m.text_secondary,
    '--border': m.border,
  }
}

export const THEME_VAR_KEYS = Object.keys(
  themeVars({
    accent: '#000000',
    bg: '#000000',
    surface: '#000000',
    surface_raised: '#000000',
    chrome_bg: '#000000',
    content_bg: '#000000',
    text_primary: '#000000',
    text_secondary: '#000000',
    border: '#000000',
  }),
)

/** Built-in defaults (mirrors tokens.css), the seed for a new theme. */
export function defaultModeTokens(dark: boolean): ModeTokens {
  return dark
    ? {
        accent: '#6b77f8',
        bg: '#1b1b21',
        surface: '#33333d',
        surface_raised: '#41414c',
        chrome_bg: '#26262e',
        content_bg: '#24242c',
        text_primary: '#eeedf0',
        text_secondary: '#a4a3b2',
        border: '#3a3a43',
      }
    : {
        accent: '#6b77f8',
        bg: '#f7f6f3',
        surface: '#ffffff',
        surface_raised: '#fafaf9',
        chrome_bg: '#eceae3',
        content_bg: '#ffffff',
        text_primary: '#1a1916',
        text_secondary: '#5c5a54',
        border: '#e4e2dc',
      }
}

/** Display labels for the editable tokens, in editor order. */
export const TOKEN_FIELDS: ReadonlyArray<{ key: keyof ModeTokens; label: string }> = [
  { key: 'accent', label: 'Accent' },
  { key: 'bg', label: 'Background' },
  { key: 'surface', label: 'Surface' },
  { key: 'surface_raised', label: 'Raised surface' },
  { key: 'chrome_bg', label: 'Chrome' },
  { key: 'content_bg', label: 'Content' },
  { key: 'text_primary', label: 'Text' },
  { key: 'text_secondary', label: 'Secondary text' },
  { key: 'border', label: 'Border' },
]
