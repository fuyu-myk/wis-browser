// Workspace background CSS (`features/workspaces/lib/background.ts`): gradient stops and opacity,
// the image dim overlay, image fit, and the editor's seed gradients.

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, test } from 'node:test'

import { backgroundCss, defaultGradient, fitCss, gradientCss } from '../../src/features/workspaces/lib/background.ts'

const spec = (opacity, stops = [
  { color: '#ff0000', position: 0 },
  { color: '#0000ff', position: 100 },
]) => ({ angle: 90, opacity, stops })

describe('gradients', () => {
  test('stops become rgba colours at their positions along the angle', () => {
    assert.equal(
      gradientCss(spec(100)),
      'linear-gradient(90deg, rgba(255, 0, 0, 1.000) 0%, rgba(0, 0, 255, 1.000) 100%)',
    )
  })

  test('a stop alpha byte is scaled by the whole-gradient opacity', () => {
    const css = gradientCss(spec(50, [{ color: '#10203080', position: 25 }]))
    assert.equal(css, 'linear-gradient(90deg, rgba(16, 32, 48, 0.251) 25%)')
  })

  test('opacity is clamped to 0..100 percent', () => {
    assert.match(gradientCss(spec(250)), /rgba\(255, 0, 0, 1\.000\)/)
    assert.match(gradientCss(spec(-20)), /rgba\(255, 0, 0, 0\.000\)/)
  })

  test('a gradient saved before opacity existed renders fully opaque', () => {
    assert.match(gradientCss(spec(undefined)), /rgba\(255, 0, 0, 1\.000\)/)
  })

  test('a gradient background is the gradient itself', () => {
    assert.equal(backgroundCss({ gradient: spec(100) }), gradientCss(spec(100)))
  })
})

describe('images', () => {
  const image = (dim) => ({ image: { asset: 'abc123', fit: 'cover', dim, blur: 0 } })

  test('an image is layered under a black dim overlay and served from the wallpaper store', () => {
    assert.equal(
      backgroundCss(image(30)),
      'linear-gradient(rgba(0, 0, 0, 0.30), rgba(0, 0, 0, 0.30)), url("wis-asset://wallpaper/abc123")',
    )
  })

  test('the dim overlay is capped at 80 percent and never negative', () => {
    assert.match(backgroundCss(image(100)), /rgba\(0, 0, 0, 0\.80\)/)
    assert.match(backgroundCss(image(-5)), /rgba\(0, 0, 0, 0\.00\)/)
  })

  test('each fit maps to its size and repeat', () => {
    assert.deepEqual(fitCss('cover'), { size: 'cover', repeat: 'no-repeat' })
    assert.deepEqual(fitCss('contain'), { size: 'contain', repeat: 'no-repeat' })
    assert.deepEqual(fitCss('tile'), { size: 'auto', repeat: 'repeat' })
  })
})

describe('seed gradients', () => {
  // `--wp-grad` in tokens.css is the default wallpaper each seed starts from.
  const tokens = readFileSync(new URL('../../src/styles/tokens.css', import.meta.url), 'utf8')
  const wallpaper = (selector) => {
    const block = tokens.slice(tokens.indexOf(selector))
    const css = block.match(/--wp-grad:\s*linear-gradient\(([^;]+)\);/)[1]
    const [angle, ...stops] = css.split(',').map((s) => s.trim())
    return {
      angle: parseInt(angle, 10),
      opacity: 100,
      stops: stops.map((s) => {
        const [color, position] = s.split(/\s+/)
        return { color: color.toLowerCase(), position: parseInt(position, 10) }
      }),
    }
  }

  test('the light and dark seeds match the default wallpaper of each mode', () => {
    assert.deepEqual(defaultGradient(false), wallpaper('.light, [data-mode="light"]'))
    assert.deepEqual(defaultGradient(true), wallpaper('.dark, [data-mode="dark"]'))
  })

  test('each call returns a fresh spec the editor can change', () => {
    const a = defaultGradient(true)
    a.stops[0].color = '#000000'
    assert.equal(defaultGradient(true).stops[0].color, '#1a1730')
  })
})
