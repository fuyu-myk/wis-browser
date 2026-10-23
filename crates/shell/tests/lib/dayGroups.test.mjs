// Lists grouped by the local day of each entry (`features/history/dayGroups.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { groupByDay, hostOf } from '../../src/features/history/dayGroups.ts'

const at = (daysAgo, hour) => {
  const date = new Date()
  date.setDate(date.getDate() - daysAgo)
  date.setHours(hour, 0, 0, 0)
  return { id: `${daysAgo}-${hour}`, at: date.toISOString() }
}

test('newest-first entries fall into one group per day, labelled for the last two', () => {
  const groups = groupByDay([at(0, 9), at(0, 8), at(1, 20), at(9, 7)], (entry) => entry.at)
  assert.deepEqual(
    groups.map((group) => group.entries.map((entry) => entry.id)),
    [['0-9', '0-8'], ['1-20'], ['9-7']],
  )
  assert.equal(groups[0].label, 'Today')
  assert.equal(groups[1].label, 'Yesterday')
  assert.notEqual(groups[2].label, 'Yesterday')
})

test('a host reads without www, and anything else as given', () => {
  assert.equal(hostOf('https://www.example.com/a'), 'example.com')
  assert.equal(hostOf('not a url'), 'not a url')
})
