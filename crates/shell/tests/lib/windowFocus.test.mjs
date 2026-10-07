// Whether this webview's OS window has focus, which gates app-wide broadcast
// events to the window the user is in (`lib/windowFocus.ts`).

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { inFreshProcess, srcUrl } from '../support/l3-harness.mjs'

/**
 * Load the module in a fresh process (it starts tracking as it loads), then
 * replay `steps`. Returns the focus state after loading and after each step,
 * and the IPC calls made.
 *
 * `os` is what the window's isFocused query answers: true, false, 'pending'
 * (never answers) or 'fails'. A step is 'os-focus' or 'os-blur' (a Tauri
 * window event) or 'focus' or 'pointerdown' (a DOM event on the webview).
 */
function run({ os = true, documentFocused = false, subscribeFails = false, steps = [] } = {}) {
  return inFreshProcess(
    async ({ installTauri, settle }, arg) => {
      const ipc = installTauri({ label: 'main' })
      globalThis.document = { hasFocus: () => arg.documentFocused }
      ipc.respond('plugin:window|is_focused', () => {
        if (arg.os === 'pending') return new Promise(() => {})
        if (arg.os === 'fails') throw new Error('ipc down')
        return arg.os
      })
      if (arg.subscribeFails) {
        ipc.respond('plugin:event|listen', () => {
          throw new Error('ipc down')
        })
      }
      const { isWindowFocused } = await import(arg.module)
      await settle()
      const states = [isWindowFocused()]
      for (const step of arg.steps) {
        if (step === 'os-focus') ipc.deliver('tauri://focus', null)
        else if (step === 'os-blur') ipc.deliver('tauri://blur', null)
        else window.dispatchEvent(new Event(step))
        states.push(isWindowFocused())
      }
      return { states, calls: ipc.calls }
    },
    { module: srcUrl('lib/windowFocus.ts'), os, documentFocused, subscribeFails, steps },
  )
}

test('a window counts as focused until the OS answers', () => {
  assert.deepEqual(run({ os: 'pending' }).states, [true])
})

test('an unfocused answer from the OS marks the window unfocused', () => {
  assert.deepEqual(run({ os: false }).states, [false])
})

test('a document that holds focus outweighs an unfocused answer from the OS', () => {
  assert.deepEqual(run({ os: false, documentFocused: true }).states, [true])
})

test("it asks about its own window by label and follows that window's focus and blur", () => {
  const { calls } = run()
  assert.deepEqual(
    calls.filter((c) => c.cmd === 'plugin:window|is_focused').map((c) => c.args),
    [{ label: 'main' }],
  )
  const listens = calls.filter((c) => c.cmd === 'plugin:event|listen').map((c) => c.args)
  assert.deepEqual(
    listens.map(({ event, target }) => ({ event, target })),
    [
      { event: 'tauri://focus', target: { kind: 'Window', label: 'main' } },
      { event: 'tauri://blur', target: { kind: 'Window', label: 'main' } },
    ],
  )
})

test('OS focus and blur events keep the state current', () => {
  const { states } = run({ steps: ['os-blur', 'os-focus', 'os-blur'] })
  assert.deepEqual(states, [true, false, true, false])
})

test('a DOM focus or a pointer press inside the webview marks the window focused', () => {
  const { states } = run({ os: false, steps: ['focus', 'os-blur', 'pointerdown'] })
  assert.deepEqual(states, [false, true, false, true])
})

test('IPC failures are swallowed and leave the window treated as focused', () => {
  assert.deepEqual(run({ os: 'fails', subscribeFails: true }).states, [true])
})
