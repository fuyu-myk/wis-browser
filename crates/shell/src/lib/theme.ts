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

/** CSS variables for one mode's tokens. The primary scale derives from the
 * accent: lighter mixes toward white above 500, darker toward black below. */
export function themeVars(m: ModeTokens): Record<string, string> {
  const accent = m.accent.slice(0, 7)
  return {
    '--primary': accent,
    '--primary-50': mixHex(accent, '#ffffff', 0.92),
    '--primary-100': mixHex(accent, '#ffffff', 0.84),
    '--primary-200': mixHex(accent, '#ffffff', 0.66),
    '--primary-300': mixHex(accent, '#ffffff', 0.46),
    '--primary-400': mixHex(accent, '#ffffff', 0.24),
    '--primary-500': accent,
    '--primary-600': mixHex(accent, '#000000', 0.18),
    '--primary-700': mixHex(accent, '#000000', 0.36),
    '--primary-800': mixHex(accent, '#000000', 0.52),
    '--primary-900': mixHex(accent, '#000000', 0.66),
    '--bg': m.bg,
    '--bg-subtle': mixHex(m.bg.slice(0, 7), m.text_primary.slice(0, 7), 0.04),
    '--surface': m.surface,
    '--surface-raised': m.surface_raised,
    '--chrome-bg': m.chrome_bg,
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
