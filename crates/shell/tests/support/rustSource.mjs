// Page scripts ship as string literals in the Rust sources. The tests read
// them from there, so they always run the text that ships.

import { readFileSync } from 'node:fs'

const SRC = new URL('../../src-tauri/src/', import.meta.url)

function read(file) {
  return readFileSync(new URL(file, SRC), 'utf8')
}

/** The body of `const NAME: &str = r#"..."#;` in `file`. */
export function rawStringConst(file, name) {
  const source = read(file)
  const at = source.indexOf(`const ${name}: &str = r#"`)
  if (at < 0) throw new Error(`${name} not found in ${file}`)
  const open = source.indexOf('r#"', at) + 3
  return source.slice(open, source.indexOf('"#;', open))
}

/**
 * The `format!` template inside `fn NAME` in `file`, with its doubled braces
 * unescaped and each `{key}` replaced by `values[key]`.
 */
export function formatTemplate(file, fnName, values) {
  const source = read(file)
  const at = source.indexOf(`fn ${fnName}`)
  if (at < 0) throw new Error(`fn ${fnName} not found in ${file}`)
  const open = source.indexOf('r#"', at) + 3
  const body = source.slice(open, source.indexOf('"#', open))
  let text = body.replace(/\{\{/g, '{').replace(/\}\}/g, '}')
  for (const [key, value] of Object.entries(values)) text = text.replace(`{${key}}`, value)
  return text
}
