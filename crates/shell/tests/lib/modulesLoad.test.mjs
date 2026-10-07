// Every lib module loads under Node, which also puts each one in the coverage
// report: a module no test imports would otherwise be missing from it rather
// than counted as untested.

import assert from 'node:assert/strict'
import { readdirSync } from 'node:fs'
import { test } from 'node:test'

const LIB = new URL('../../src/lib/', import.meta.url)

// Touch `window` as they load; their own tests stub it first.
const NEEDS_A_WINDOW = new Set(['windowFocus.ts'])

const modules = readdirSync(LIB).filter((f) => f.endsWith('.ts') && !NEEDS_A_WINDOW.has(f))

for (const file of modules) {
  test(`lib/${file} loads`, async () => {
    const mod = await import(new URL(file, LIB).href)
    assert.ok(Object.keys(mod).length > 0, 'exports something')
  })
}
