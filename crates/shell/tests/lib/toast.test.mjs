// Raising a toast from the chrome or from another webview (`lib/toast.ts`).

import assert from 'node:assert/strict'
import { beforeEach, test } from 'node:test'

import * as toast from '../../src/lib/toast.ts'
import { filesSpelling, installTauri } from '../support/l3-harness.mjs'

let ipc
beforeEach(() => {
  ipc = installTauri()
})

test('a chrome toast is a DOM event carrying the message', () => {
  const seen = []
  window.addEventListener(toast.TOAST_REQUEST_EVENT, (e) => seen.push(e.detail))
  toast.showToast('Link copied')
  assert.deepEqual(seen, [{ message: 'Link copied' }])
})

test('a chrome toast stays inside the webview and makes no IPC call', () => {
  toast.showToast('Link copied')
  assert.deepEqual(ipc.calls, [])
})

test('a remote toast is a Tauri event addressed to a window by label', async () => {
  await toast.showRemoteToast('main-2', 'Image copied')
  assert.deepEqual(ipc.argsOf('plugin:event|emit'), [
    { event: toast.TOAST_REMOTE_EVENT, payload: { window: 'main-2', message: 'Image copied' } },
  ])
})

test('a remote toast that fails to send resolves quietly', async () => {
  ipc.respond('plugin:event|emit', () => {
    throw new Error('ipc down')
  })
  assert.equal(await toast.showRemoteToast('main', 'Saved'), undefined)
})

test('every toast event name is distinct and spelled only in this contract', () => {
  const names = [
    toast.TOAST_REQUEST_EVENT,
    toast.TOAST_SHOW_EVENT,
    toast.TOAST_REMOTE_EVENT,
    toast.TOAST_CLOSED_EVENT,
    toast.TOAST_READY_EVENT,
  ]
  assert.equal(new Set(names).size, names.length)
  for (const name of names) assert.deepEqual(filesSpelling(name), ['lib/toast.ts'], name)
})
