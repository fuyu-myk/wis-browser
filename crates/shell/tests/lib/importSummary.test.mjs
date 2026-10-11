// Result lines shown after a bookmark import (`lib/importSummary.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { bookmarkImportSummary, historyImportSummary } from '../../src/lib/importSummary.ts'

const stats = (added, folders, skipped) => ({
  bookmarks_added: added,
  folders_added: folders,
  bookmarks_skipped: skipped,
})

test('an import that added nothing says why', () => {
  assert.equal(bookmarkImportSummary(stats(0, 0, 0), 'None here.'), 'None here.')
  assert.equal(
    bookmarkImportSummary(stats(0, 0, 3), 'None here.'),
    'Nothing new: 3 bookmarks already saved.',
  )
})

test('added bookmarks, folders and skipped duplicates are counted', () => {
  assert.equal(bookmarkImportSummary(stats(1, 0, 0), ''), 'Imported 1 bookmark.')
  assert.equal(
    bookmarkImportSummary(stats(5, 2, 1), ''),
    'Imported 5 bookmarks and 2 folders (1 duplicate skipped).',
  )
})

test('a history import counts new pages and pages already there', () => {
  const h = (added, merged) => ({ added, merged })
  assert.equal(historyImportSummary(h(0, 0), 'No history.'), 'No history.')
  assert.equal(historyImportSummary(h(0, 2), ''), 'Nothing new in history: 2 pages already there.')
  assert.equal(historyImportSummary(h(1, 0), ''), 'Imported 1 page of history.')
  assert.equal(
    historyImportSummary(h(40, 3), ''),
    'Imported 40 pages of history (3 pages already there).',
  )
})
