// Formatting and ranges for the usage panel and page (`lib/usage.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import {
  clampUsagePanelWidth,
  dailyAverage,
  dayLabel,
  durationTicks,
  firstWeekday,
  formatDuration,
  hourLabel,
  changeLabel,
  localDay,
  sharePercent,
  shiftWeek,
  stepSlot,
  weekOf,
  weekChange,
  weekRangeLabel,
  weekTotals,
  weekdayLabel,
  weeklyAverage,
} from '../../src/lib/usage.ts'

const HOUR = 3600

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

test('weeks shift by whole weeks across months and years', () => {
  assert.deepEqual(shiftWeek('2026-10-05', -1), { from: '2026-09-28', to: '2026-10-04' })
  assert.deepEqual(shiftWeek('2025-12-29', 1), { from: '2026-01-05', to: '2026-01-11' })
  assert.deepEqual(shiftWeek('2026-03-23', 1), { from: '2026-03-30', to: '2026-04-05' })
  assert.deepEqual(shiftWeek('2026-10-05', 0), { from: '2026-10-05', to: '2026-10-11' })
})

test('duration ticks use round steps and cover the peak', () => {
  assert.deepEqual(durationTicks(0), [0, 300])
  assert.deepEqual(durationTicks(25 * 60), [0, 600, 1200, 1800])
  assert.deepEqual(durationTicks(HOUR), [0, 900, 1800, 2700, 3600])
  assert.deepEqual(durationTicks(5 * HOUR), [0, 2 * HOUR, 4 * HOUR, 6 * HOUR])
  assert.deepEqual(durationTicks(24 * HOUR), [0, 6 * HOUR, 12 * HOUR, 18 * HOUR, 24 * HOUR])
})

test('a smaller chart can ask for fewer duration intervals', () => {
  assert.deepEqual(durationTicks(HOUR, 2), [0, 1800, 3600])
  assert.deepEqual(durationTicks(20 * 60, 2), [0, 600, 1200])
  assert.deepEqual(durationTicks(5 * HOUR, 2), [0, 3 * HOUR, 6 * HOUR])
  assert.deepEqual(durationTicks(0, 2), [0, 300])
})

test('week ranges name the year only outside the current one', () => {
  const now = new Date(2026, 9, 11)
  const label = (from, to) => weekRangeLabel(from, to, now, 'en-US').replace(/\s/g, ' ')
  assert.equal(label('2026-10-05', '2026-10-11'), 'Oct 5 – 11')
  assert.equal(label('2026-09-28', '2026-10-04'), 'Sep 28 – Oct 4')
  assert.equal(label('2025-12-29', '2026-01-04'), 'Dec 29, 2025 – Jan 4, 2026')
  assert.equal(dayLabel('2026-10-05', 'en-US'), 'Mon, Oct 5')
})

test('the daily average counts only days that have started', () => {
  const week = ['05', '06', '07', '08', '09', '10', '11'].map((d) => ({
    day: `2026-10-${d}`,
    seconds: HOUR,
  }))
  assert.equal(dailyAverage(week, '2026-10-20'), HOUR)
  assert.equal(dailyAverage(week, '2026-10-07'), HOUR)
  assert.equal(dailyAverage(week.map((d, i) => ({ ...d, seconds: i < 3 ? 2 * HOUR : 0 })), '2026-10-07'), 2 * HOUR)
  assert.equal(dailyAverage(week, '2026-10-01'), 0)
})

test('chart keys step within the slots', () => {
  assert.equal(stepSlot('ArrowLeft', 0, 6), 0)
  assert.equal(stepSlot('ArrowLeft', 3, 6), 2)
  assert.equal(stepSlot('ArrowRight', 6, 6), 6)
  assert.equal(stepSlot('ArrowRight', 2, 6), 3)
  assert.equal(stepSlot('Home', 4, 6), 0)
  assert.equal(stepSlot('End', 1, 6), 6)
  assert.equal(stepSlot('Enter', 1, 6), null)
})

const daysFrom = (start, seconds) =>
  seconds.map((value, i) => {
    const date = new Date(2026, 8, start + i)
    return { day: localDay(date), seconds: value }
  })

test('weekly totals sum each run of seven days under its first day', () => {
  const days = daysFrom(28, [1, 2, 3, 4, 5, 6, 7, 10, 10, 10, 10, 10, 10, 10])
  assert.deepEqual(weekTotals(days), [
    { day: '2026-09-28', seconds: 28 },
    { day: '2026-10-05', seconds: 70 },
  ])
  assert.deepEqual(weekTotals([]), [])
})

test('a week is compared with the same weekdays of the week before', () => {
  const before = daysFrom(28, [HOUR, HOUR, HOUR, 5 * HOUR, 0, 0, 0])
  const week = daysFrom(35, [2 * HOUR, HOUR, 0, 0, 0, 0, 0])
  assert.equal(weekChange(week, before, '2026-10-07'), 0)
  assert.equal(weekChange(week, before, '2026-10-06'), 0.5)
  assert.equal(weekChange(week, before, '2026-10-20'), -0.625)
  assert.equal(weekChange(week, daysFrom(28, [0, 0, 0, 0, 0, 0, 0]), '2026-10-20'), null)
  assert.equal(weekChange(week, before, '2026-10-01'), null)
})

test('changes read as more, less or the same', () => {
  assert.equal(changeLabel(0.123, true), '12% more than this time last week')
  assert.equal(changeLabel(-0.5, false), '50% less than the week before')
  assert.equal(changeLabel(0.001, true), 'Same as this time last week')
  assert.equal(changeLabel(-0.004, false), 'Same as the week before')
})

test('the weekly average skips the running week and weeks before tracking began', () => {
  const weeks = [0, 0, 10, 20, 30, 40].map((seconds, i) => ({ day: `w${i}`, seconds }))
  assert.equal(weeklyAverage(weeks, true), 20)
  assert.equal(weeklyAverage(weeks, false), 25)
  assert.equal(weeklyAverage(weeks.map((w) => ({ ...w, seconds: 0 })), false), null)
  assert.equal(weeklyAverage([{ day: 'w0', seconds: 50 }], true), null)
})

test('weeks can start on any day', () => {
  assert.deepEqual(weekOf(new Date(2026, 9, 11), 0), { from: '2026-10-11', to: '2026-10-17' })
  assert.deepEqual(weekOf(new Date(2026, 9, 10), 0), { from: '2026-10-04', to: '2026-10-10' })
  assert.deepEqual(weekOf(new Date(2026, 9, 11), 6), { from: '2026-10-10', to: '2026-10-16' })
  assert.deepEqual(shiftWeek('2026-10-11', -1), { from: '2026-10-04', to: '2026-10-10' })
})

test('the first weekday follows the setting, else the region', () => {
  assert.equal(firstWeekday('monday', 'en-US'), 1)
  assert.equal(firstWeekday('sunday', 'en-GB'), 0)
  assert.equal(firstWeekday('auto', 'en-US'), 0)
  assert.equal(firstWeekday('auto', 'en-GB'), 1)
  assert.equal(firstWeekday('auto', 'ar-EG'), 6)
  assert.equal(firstWeekday('auto', 'not a locale!'), 1)
})
