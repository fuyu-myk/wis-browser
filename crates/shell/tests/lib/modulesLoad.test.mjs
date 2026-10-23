// Every lib module loads under Node, which also puts each one in the coverage
// report: a module no test imports would otherwise be missing from it rather
// than counted as untested.

import assert from 'node:assert/strict'
import { readdirSync } from 'node:fs'
import { test } from 'node:test'

const SRC = new URL('../../src/', import.meta.url)

// Touch `window` as they load; their own tests stub it first.
const NEEDS_A_WINDOW = new Set(['lib/windowFocus.ts'])

// Lib modules: those in `src/lib`, in a feature's `lib/`, or at a flat
// feature's root but for its hooks.
const modules = readdirSync(SRC, { recursive: true })
  .map((f) => f.split('\\').join('/'))
  .filter((f) => f.endsWith('.ts') && !f.endsWith('.d.ts') && !NEEDS_A_WINDOW.has(f))
  .filter((f) => {
    const parts = f.split('/')
    if (parts[0] === 'lib') return parts.length === 2
    if (parts[0] !== 'features') return false
    if (parts.length === 4) return parts[2] === 'lib'
    return parts.length === 3 && !parts[2].startsWith('use')
  })

for (const file of modules) {
  test(`${file} loads`, async () => {
    const mod = await import(new URL(file, SRC).href)
    assert.ok(Object.keys(mod).length > 0, 'exports something')
  })
}
