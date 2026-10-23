// Formatting and filtering for the downloads button, popup and page
// (`features/downloads/downloads.ts`).

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import * as dl from '../../src/features/downloads/downloads.ts'
import { filesSpelling, readSource } from '../support/l3-harness.mjs'

const KB = 1024
const MB = 1024 * KB

function item(over = {}) {
  return {
    id: 1,
    url: 'https://www.example.com/files/report.pdf',
    filename: 'Report.pdf',
    path: '/Users/me/Downloads/Report.pdf',
    receivedBytes: 0,
    totalBytes: null,
    status: 'completed',
    private: false,
    startedAtMs: 0,
    ...over,
  }
}

describe('byte sizes', () => {
  test('are shown in bytes below one kilobyte', () => {
    assert.equal(dl.formatBytes(0), '0 B')
    assert.equal(dl.formatBytes(1023), '1023 B')
  })

  test('carry one decimal below 100 of a unit', () => {
    assert.equal(dl.formatBytes(KB), '1.0 KB')
    assert.equal(dl.formatBytes(1536), '1.5 KB')
    assert.equal(dl.formatBytes(5.25 * MB), '5.3 MB')
  })

  test('are whole numbers from 100 of a unit up', () => {
    assert.equal(dl.formatBytes(100 * KB), '100 KB')
    assert.equal(dl.formatBytes(700 * MB), '700 MB')
  })

  test('climb through the units and stop at terabytes', () => {
    assert.equal(dl.formatBytes(MB), '1.0 MB')
    assert.equal(dl.formatBytes(2 * 1024 * MB), '2.0 GB')
    assert.equal(dl.formatBytes(3 * 1024 * 1024 * MB), '3.0 TB')
    assert.equal(dl.formatBytes(2048 * 1024 * 1024 * MB), '2048 TB')
  })

  test('a size that rounds up to the next unit reads in that unit', () => {
    assert.equal(dl.formatBytes(1048575), '1.0 MB')
    assert.equal(dl.formatBytes(1073741823), '1.0 GB')
  })

  test('a size that rounds up to 100 drops its decimal', () => {
    assert.equal(dl.formatBytes(102350), '100 KB')
  })
})

describe('the source host', () => {
  test('drops a leading www. and keeps the port', () => {
    assert.equal(dl.downloadHost('https://www.example.com/a.zip'), 'example.com')
    assert.equal(dl.downloadHost('http://cdn.example.com:8080/a.zip'), 'cdn.example.com:8080')
    assert.equal(dl.downloadHost('https://www2.example.com/a.zip'), 'www2.example.com')
  })

  test('falls back to the raw string when it is not a URL', () => {
    assert.equal(dl.downloadHost('not a url'), 'not a url')
  })
})

describe('searching downloads', () => {
  const report = item()

  test('a blank search matches everything', () => {
    assert.equal(dl.matchesDownloadSearch(report, ''), true)
    assert.equal(dl.matchesDownloadSearch(report, '   '), true)
  })

  test('matches the filename or URL, ignoring case and surrounding space', () => {
    assert.equal(dl.matchesDownloadSearch(report, '  REPORT '), true)
    assert.equal(dl.matchesDownloadSearch(report, '/files/'), true)
    assert.equal(dl.matchesDownloadSearch(report, 'invoice'), false)
  })

  test('matches the host as displayed, which for an IDN differs from the URL', () => {
    const idn = item({ url: 'https://bücher.example/x.epub', filename: 'x.epub' })
    assert.equal(dl.matchesDownloadSearch(idn, 'xn--bcher'), true)
  })
})

test('each status has its row label', () => {
  assert.equal(
    dl.statusLabel(item({ status: 'in_progress', receivedBytes: 1536 })),
    'Downloading, 1.5 KB so far',
  )
  assert.equal(dl.statusLabel(item({ status: 'completed', receivedBytes: 5 * MB })), '5.0 MB')
  assert.equal(dl.statusLabel(item({ status: 'failed' })), 'Failed')
  assert.equal(dl.statusLabel(item({ status: 'cancelled' })), 'Cancelled')
})

test('only an in-progress download counts as in progress', () => {
  const states = ['in_progress', 'completed', 'failed', 'cancelled']
  assert.deepEqual(
    states.filter((status) => dl.isInProgress(item({ status }))),
    ['in_progress'],
  )
})

test('the change event is the one data/downloads.rs emits', () => {
  const rust = readSource('src-tauri/src/data/downloads.rs')
  assert.ok(rust.includes(`pub const DOWNLOADS_CHANGED_EVENT: &str = "${dl.DOWNLOADS_CHANGED_EVENT}";`))
})

test('the popup events are distinct and spelled only in this contract', () => {
  const names = [dl.DOWNLOADS_OPEN_EVENT, dl.DOWNLOADS_CLOSED_EVENT, dl.DOWNLOADS_SHOWN_EVENT]
  assert.equal(new Set([...names, dl.DOWNLOADS_CHANGED_EVENT]).size, 4)
  for (const name of names) assert.deepEqual(filesSpelling(name), ['features/downloads/downloads.ts'], name)
})
