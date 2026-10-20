// Shared by the lib tests: a fake Tauri IPC behind a fake chrome window, a
// fresh-process runner for modules that act as they load, and readers for the
// sources a cross-file contract spans.
//
// `@tauri-apps/api` reaches the backend only through
// `window.__TAURI_INTERNALS__.invoke` and `.transformCallback`, plus
// `window.__TAURI_EVENT_PLUGIN_INTERNALS__.unregisterListener` on unlisten.
// The fake records every invoke, runs the event plugin's listen, unlisten and
// emit as a tiny in-memory bus, and lets a test answer or fail any command.

import { execFileSync } from 'node:child_process'
import { readdirSync, readFileSync } from 'node:fs'

const SHELL = new URL('../../', import.meta.url)
const REGISTER = new URL('./register.mjs', import.meta.url).href

/**
 * Install a fresh `globalThis.window` (an EventTarget, so DOM events work)
 * wired to a fake Tauri backend, and return its controls.
 */
export function installTauri({ label = 'main' } = {}) {
  const calls = []
  const callbacks = new Map()
  const listeners = new Map()
  const responders = new Map()
  let nextId = 1

  function deliver(event, payload) {
    for (const [eventId, l] of [...listeners]) {
      if (l.event === event) callbacks.get(l.handler)?.({ event, id: eventId, payload })
    }
  }

  const eventPlugin = {
    'plugin:event|listen': ({ event, target, handler }) => {
      const eventId = nextId++
      listeners.set(eventId, { event, target, handler })
      return eventId
    },
    'plugin:event|unlisten': ({ eventId }) => {
      listeners.delete(eventId)
    },
    'plugin:event|emit': ({ event, payload }) => deliver(event, payload),
  }

  const win = Object.assign(new EventTarget(), {
    __TAURI_INTERNALS__: {
      metadata: { currentWindow: { label }, currentWebview: { windowLabel: label, label } },
      transformCallback(cb) {
        const id = nextId++
        callbacks.set(id, cb)
        return id
      },
      unregisterCallback(id) {
        callbacks.delete(id)
      },
      async invoke(cmd, args = {}) {
        calls.push({ cmd, args })
        const respond = responders.get(cmd) ?? eventPlugin[cmd]
        return respond ? respond(args) : null
      },
    },
    __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener() {} },
  })
  globalThis.window = win

  return {
    window: win,
    calls,
    /** The arguments of every invoke of `cmd`, in order. */
    argsOf: (cmd) => calls.filter((c) => c.cmd === cmd).map((c) => c.args),
    /** Answer `cmd` with `fn(args)`; a throw or rejection fails the invoke. */
    respond: (cmd, fn) => responders.set(cmd, fn),
    /** Deliver a backend event to every live listener of `event`. */
    deliver,
    /** Live listeners of `event`. */
    listenerCount: (event) => [...listeners.values()].filter((l) => l.event === event).length,
  }
}

/**
 * Run `scenario(harness, arg)` in a fresh Node process and return its JSON
 * result. For modules that act as they load: each run loads them anew under
 * their real URL, so coverage merges across runs, which it does not for
 * cache-busted copies. `scenario` travels as source text, so it may use only
 * its parameters (this module's exports and the JSON `arg`), not outer names.
 */
export function inFreshProcess(scenario, arg) {
  const script = [
    `const harness = await import(${JSON.stringify(import.meta.url)})`,
    `const result = await (${scenario})(harness, ${JSON.stringify(arg)})`,
    'process.stdout.write(JSON.stringify(result ?? null))',
  ].join('\n')
  const out = execFileSync(
    process.execPath,
    ['--import', REGISTER, '--input-type=module', '--eval', script],
    { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] },
  )
  return JSON.parse(out)
}

/** A module under `crates/shell/src/`, as an importable URL. */
export function srcUrl(path) {
  return new URL(`src/${path}`, SHELL).href
}

/** Let pending promise chains run to completion. */
export function settle() {
  return new Promise((resolve) => setImmediate(resolve))
}

/** A source file under `crates/shell/`, as text. */
export function readSource(path) {
  return readFileSync(new URL(path, SHELL), 'utf8').replace(/\r\n/g, '\n')
}

/** The lines inside the top-level `{ ... }` block that `opener` starts, uncommented. */
function blockLines(source, opener) {
  const at = source.indexOf(opener)
  if (at < 0) throw new Error(`${opener} not found`)
  return source
    .slice(at + opener.length, source.indexOf('\n}', at))
    .split('\n')
    .map((line) => line.replace(/\/\/.*$/, '').trim())
}

/** Field names of `interface NAME { ... }` in TypeScript source. */
export function interfaceFields(source, name) {
  return blockLines(source, `interface ${name} {`)
    .map((line) => line.match(/^(\w+)\??:/)?.[1])
    .filter(Boolean)
}

/**
 * Field names of `struct NAME { ... }` in Rust source, as serde serialises
 * them: as written, or camelCased under `rename_all = "camelCase"`.
 */
export function serializedStructFields(source, name) {
  const opener = `struct ${name} {`
  const before = source.slice(0, source.indexOf(opener)).split('\n')
  const attrs = []
  for (let i = before.length - 2; i >= 0 && before[i].trim().startsWith('#['); i -= 1) {
    attrs.push(before[i])
  }
  const renameAll = attrs.join('\n').match(/rename_all\s*=\s*"(\w+)"/)?.[1]
  if (renameAll && renameAll !== 'camelCase') throw new Error(`rename_all = ${renameAll} unsupported`)
  return blockLines(source, opener)
    .map((line) => line.match(/^(?:pub(?:\([^)]*\))?\s+)?(\w+):/)?.[1])
    .filter(Boolean)
    .map((field) => (renameAll ? field.replace(/_([a-z])/g, (_, c) => c.toUpperCase()) : field))
}

/**
 * Frontend sources (`src/`, relative paths) that spell `value` as a quoted
 * string literal.
 */
export function filesSpelling(value) {
  const src = new URL('src/', SHELL)
  const quoted = [`'${value}'`, `"${value}"`]
  return readdirSync(src, { recursive: true })
    .filter((f) => /\.tsx?$/.test(f))
    .filter((f) => {
      const text = readFileSync(new URL(f, src), 'utf8')
      return quoted.some((q) => text.includes(q))
    })
    .map((f) => f.split('\\').join('/'))
    .sort()
}
