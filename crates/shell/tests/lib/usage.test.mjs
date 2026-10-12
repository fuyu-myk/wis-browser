// Formatting and ranges for the usage panel (`lib/usage.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  clampUsagePanelWidth,
  formatDuration,
  hourLabel,
  localDay,
  sharePercent,
  weekOf,
  weekdayLabel,
} from '../../src/lib/usage.ts'

test('durations read in the largest sensible unit', () => {
  assert.equal(formatDuration(0), '0s')
  assert.equal(formatDuration(45), '45s')
  assert.equal(formatDuration(60), '1m')
  assert.equal(formatDuration(3599), '59m')
  assert.equal(formatDuration(3600), '1h')
  assert.equal(formatDuration(3600 + 5 * 60), '1h 5m')
  assert.equal(formatDuration(-3), '0s')
})

test('days are local calendar dates', () => {
  assert.equal(localDay(new Date(2026, 0, 5, 23, 59)), '2026-01-05')
  assert.equal(localDay(new Date(2026, 9, 11, 0, 0)), '2026-10-11')
})

test('a week runs Monday to Sunday around the day', () => {
  assert.deepEqual(weekOf(new Date(2026, 9, 11)), { from: '2026-10-05', to: '2026-10-11' })
  assert.deepEqual(weekOf(new Date(2026, 9, 12)), { from: '2026-10-12', to: '2026-10-18' })
  assert.deepEqual(weekOf(new Date(2026, 10, 1)), { from: '2026-10-26', to: '2026-11-01' })
})

test('shares are whole percents and any time shows at least 1', () => {
  assert.equal(sharePercent(0, 100), 0)
  assert.equal(sharePercent(1, 1000), 1)
  assert.equal(sharePercent(333, 1000), 33)
  assert.equal(sharePercent(5, 0), 0)
})

test('hours and weekdays get short labels', () => {
  assert.equal(hourLabel(0), '12 AM')
  assert.equal(hourLabel(6), '6 AM')
  assert.equal(hourLabel(12), '12 PM')
  assert.equal(hourLabel(18), '6 PM')
  assert.equal(weekdayLabel('2026-10-12'), 'Mon')
  assert.equal(weekdayLabel('2026-10-18'), 'Sun')
})

test('the panel width stays within its bounds and half the window', () => {
  assert.equal(clampUsagePanelWidth(100, 2000), 280)
  assert.equal(clampUsagePanelWidth(900, 2000), 520)
  assert.equal(clampUsagePanelWidth(400, 700), 350)
  assert.equal(clampUsagePanelWidth(Number.NaN, 2000), 280)
})
