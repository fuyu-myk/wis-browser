// The layout's rules: shared lib code never reaches into a feature, and
// features depend on each other without a cycle.

import assert from 'node:assert/strict'
import { readdirSync, readFileSync } from 'node:fs'
import { test } from 'node:test'

const SRC = new URL('../src/', import.meta.url)

const files = readdirSync(SRC, { recursive: true })
  .map((f) => f.split('\\').join('/'))
  .filter((f) => /\.tsx?$/.test(f))

const imports = (file) =>
  [...readFileSync(new URL(file, SRC), 'utf8').matchAll(/from '@\/([^']+)'/g)].map((m) => m[1])

const featureOf = (path) => (path.startsWith('features/') ? path.split('/')[1] : null)

test('shared lib code never imports a feature', () => {
  const reaching = files
    .filter((f) => f.startsWith('lib/'))
    .flatMap((f) => imports(f).filter(featureOf).map((target) => `${f} imports ${target}`))
  assert.deepEqual(reaching, [])
})

test('features depend on each other without a cycle', () => {
  const graph = new Map()
  for (const file of files) {
    const from = featureOf(file)
    if (!from) continue
    for (const target of imports(file)) {
      const to = featureOf(target)
      if (to && to !== from) graph.set(from, new Set([...(graph.get(from) ?? []), to]))
    }
  }
  const done = new Set()
  const visit = (node, path) => {
    assert.ok(!path.includes(node), `cycle: ${[...path, node].join(' then ')}`)
    if (done.has(node)) return
    for (const next of graph.get(node) ?? []) visit(next, [...path, node])
    done.add(node)
  }
  for (const node of graph.keys()) visit(node, [])
})
