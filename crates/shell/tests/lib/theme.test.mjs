// Custom theme tokens (`lib/theme.ts`): colour mixing, the primary scale
// derived from one accent, and the built-in seed tokens.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, test } from 'node:test'

import {
  THEME_VAR_KEYS,
  TOKEN_FIELDS,
  defaultModeTokens,
  mixHex,
  themeVars,
} from '../../src/lib/theme.ts'

const luminance = (hex) =>
  [1, 3, 5].reduce((sum, i) => sum + parseInt(hex.slice(i, i + 2), 16), 0)

describe('mixing', () => {
  test('the ends of the mix are the two colours', () => {
    assert.equal(mixHex('#336699', '#ffffff', 0), '#336699')
    assert.equal(mixHex('#336699', '#ffffff', 1), '#ffffff')
  })

  test('a mix rounds each channel and keeps two hex digits', () => {
    assert.equal(mixHex('#000000', '#ffffff', 0.5), '#808080')
    assert.equal(mixHex('#000000', '#ffffff', 0.02), '#050505')
    assert.equal(mixHex('#ff0000', '#0000ff', 0.25), '#bf0040')
  })
})

describe('theme variables', () => {
  const light = defaultModeTokens(false)

  test('the primary scale lightens above 500 and darkens below it, monotonically', () => {
    const vars = themeVars(light)
    const steps = [50, 100, 200, 300, 400, 500, 600, 700, 800, 900].map((s) => vars[`--primary-${s}`])
    assert.equal(vars['--primary-500'], light.accent)
    assert.equal(vars['--primary'], light.accent)
    for (let i = 1; i < steps.length; i++) {
      assert.ok(luminance(steps[i]) < luminance(steps[i - 1]), `${steps[i - 1]} then ${steps[i]}`)
    }
    assert.equal(vars['--primary-50'], mixHex(light.accent, '#ffffff', 0.92))
    assert.equal(vars['--primary-900'], mixHex(light.accent, '#000000', 0.66))
  })

  test('an accent with an alpha byte drives the scale by its colour alone', () => {
    const vars = themeVars({ ...light, accent: '#6b77f880' })
    assert.equal(vars['--primary'], '#6b77f8')
    assert.equal(vars['--primary-600'], themeVars(light)['--primary-600'])
  })

  test('the subtle background leans four percent toward the text colour', () => {
    const vars = themeVars({ ...light, bg: '#ffffffcc', text_primary: '#000000' })
    assert.equal(vars['--bg'], '#ffffffcc')
    assert.equal(vars['--bg-subtle'], '#f5f5f5')
  })

  test('surface tokens pass through, and popovers share the chrome colour', () => {
    const vars = themeVars(light)
    assert.equal(vars['--surface'], light.surface)
    assert.equal(vars['--surface-raised'], light.surface_raised)
    assert.equal(vars['--chrome-bg'], light.chrome_bg)
    assert.equal(vars['--popover'], light.chrome_bg)
    assert.equal(vars['--content-bg'], light.content_bg)
    assert.equal(vars['--text-primary'], light.text_primary)
    assert.equal(vars['--text-secondary'], light.text_secondary)
    assert.equal(vars['--border'], light.border)
  })

  test('the keys cleared on theme removal are exactly the keys a theme sets', () => {
    assert.deepEqual(THEME_VAR_KEYS, Object.keys(themeVars(light)))
    assert.equal(new Set(THEME_VAR_KEYS).size, THEME_VAR_KEYS.length)
  })
})

describe('seed tokens', () => {
  const tokens = readFileSync(new URL('../../src/styles/tokens.css', import.meta.url), 'utf8')
  const cssVar = (selector, name) => {
    const block = tokens.slice(tokens.indexOf(selector))
    return block.match(new RegExp(`--${name}:\\s*(#[0-9A-Fa-f]{6})`))[1].toLowerCase()
  }
  const NEUTRALS = {
    bg: 'bg',
    surface: 'surface',
    surface_raised: 'surface-raised',
    chrome_bg: 'chrome-bg',
    content_bg: 'content-bg',
    text_primary: 'text-primary',
    text_secondary: 'text-secondary',
    border: 'border',
  }

  test('the seeds match the built-in tokens of each mode', () => {
    for (const [dark, selector] of [
      [false, '.light, [data-mode="light"]'],
      [true, '.dark, [data-mode="dark"]'],
    ]) {
      const seed = defaultModeTokens(dark)
      for (const [key, name] of Object.entries(NEUTRALS)) {
        assert.equal(seed[key], cssVar(selector, name), `${selector} ${name}`)
      }
      assert.equal(seed.accent, cssVar(':root', 'primary-500'))
    }
  })

  test('the editor lists every token once', () => {
    const keys = TOKEN_FIELDS.map((f) => f.key)
    assert.deepEqual([...keys].sort(), Object.keys(defaultModeTokens(false)).sort())
    assert.equal(new Set(keys).size, keys.length)
  })
})
