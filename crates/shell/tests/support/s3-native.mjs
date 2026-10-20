// The snippets native code evaluates in a tab page, read from the Rust sources so tests send exactly what ships.

import { readFileSync } from 'node:fs'

const SRC = new URL('../../src-tauri/src/', import.meta.url)

/** `file` without its raw strings, which hold the page scripts themselves. */
function nativeSource(file) {
  return readFileSync(new URL(file, SRC), 'utf8')
    .replace(/\r\n/g, '\n')
    .replace(/r#"[\s\S]*?"#/g, '')
}

/** The plain Rust string literal in `file` that contains `needle`. */
export function rustLiteral(file, needle) {
  const source = nativeSource(file)
  const at = source.indexOf(needle)
  if (at < 0) throw new Error(`${needle} not found in ${file}`)
  return source.slice(source.lastIndexOf('"', at) + 1, source.indexOf('"', at + needle.length))
}

/** Whether `file` (raw strings aside) contains `text`. */
export function rustHas(file, text) {
  return nativeSource(file).includes(text)
}

/** Run a native snippet in the page, as `webview.eval` would. */
export function evalIn(window, js) {
  new Function('window', js)(window)
}
