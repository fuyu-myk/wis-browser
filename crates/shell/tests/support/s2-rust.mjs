// Reads script text out of Rust sources where `rustSource.mjs` does not reach:
// raw strings inside a function body, and files outside the shell crate.

import { readFileSync } from 'node:fs'

const SRC = new URL('../../src-tauri/src/', import.meta.url)
const CRATES = new URL('../../../', import.meta.url)

/** Source text with LF line ends, as the parsers here expect: a Windows
 *  checkout turns them into CRLF. */
function lf(text) {
  return text.replace(/\r\n/g, '\n')
}

/** A shell source file, by name under `src-tauri/src/`. */
export function shellSource(file) {
  return lf(readFileSync(new URL(file, SRC), 'utf8'))
}

/** Any file under `crates/`, e.g. `core/src/link_safety.rs`. */
export function crateSource(path) {
  return lf(readFileSync(new URL(path, CRATES), 'utf8'))
}

/** The first `r#"..."#` literal inside `fn NAME` in `file`, verbatim. */
export function rawStringInFn(file, fnName) {
  const source = shellSource(file)
  const at = source.indexOf(`fn ${fnName}(`)
  if (at < 0) throw new Error(`fn ${fnName} not found in ${file}`)
  const open = source.indexOf('r#"', at) + 3
  return source.slice(open, source.indexOf('"#', open))
}

/** The body of the `fn NAME` in `file`, up to the next top-level item. */
export function fnBody(file, fnName) {
  const source = shellSource(file)
  const at = source.indexOf(`fn ${fnName}(`)
  if (at < 0) throw new Error(`fn ${fnName} not found in ${file}`)
  const end = source.indexOf('\n}\n', at)
  return source.slice(at, end < 0 ? undefined : end)
}
