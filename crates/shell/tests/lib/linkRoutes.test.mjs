// Settings' rules for links from other apps (`lib/linkRoutes.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  MAX_LINK_ROUTES,
  looksLikeSite,
  withImportedRoutes,
  withRoute,
  withoutRoute,
} from '../../src/lib/linkRoutes.ts'

test('a rule can be added for a site, with or without a path', () => {
  for (const site of [
    'figma.com',
    'github.com/acme',
    'https://www.example.com/',
    'localhost',
    'bücher.de',
    '.figma.com',
    '*.slack.com',
    'figma.com.',
    '192.168.1.1',
  ]) {
    assert.equal(looksLikeSite(site), true, site)
  }
  for (const junk of [
    '',
    '   ',
    'figma',
    'two words.com',
    '/acme',
    'user@figma.com',
    'example.com:8080',
    'figma..com',
    '*',
    'www.*.figma.com',
    'ftp://figma.com',
  ]) {
    assert.equal(looksLikeSite(junk), false, junk)
  }
})

test('adding a site again moves it to the new workspace, and removing drops only it', () => {
  const routes = [
    { pattern: 'figma.com', workspace_id: 'work' },
    { pattern: 'github.com', workspace_id: 'home' },
  ]
  assert.deepEqual(withRoute(routes, ' Figma.com ', 'home'), [
    { pattern: 'github.com', workspace_id: 'home' },
    { pattern: 'figma.com', workspace_id: 'home' },
  ])
  assert.deepEqual(withoutRoute(routes, 'figma.com'), [{ pattern: 'github.com', workspace_id: 'home' }])
})

test('restored rules join the current ones, winning a shared site, up to the cap', () => {
  const current = [
    { pattern: 'figma.com', workspace_id: 'work' },
    { pattern: 'github.com', workspace_id: 'home' },
  ]
  assert.deepEqual(withImportedRoutes(current, [{ pattern: 'figma.com', workspace_id: 'home' }]), [
    { pattern: 'github.com', workspace_id: 'home' },
    { pattern: 'figma.com', workspace_id: 'home' },
  ])

  const many = Array.from({ length: MAX_LINK_ROUTES }, (_, i) => ({ pattern: `s${i}.com`, workspace_id: 'home' }))
  const merged = withImportedRoutes(current, many)
  assert.equal(merged.length, MAX_LINK_ROUTES)
  assert.deepEqual(merged.slice(0, 2), current)
})
