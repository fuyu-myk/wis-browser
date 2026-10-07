// Evaluates an expression against a lib module loaded in a fresh Node whose
// user agent names another OS. `lib/platform.ts` reads the platform once at
// load, so a second platform cannot be had inside the same process.

import { execFileSync } from 'node:child_process'

const REGISTER = new URL('./register.mjs', import.meta.url)
const LIB = new URL('../../src/lib/', import.meta.url)

export const WINDOWS_UA = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Edg/120.0'
export const LINUX_UA = 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15'

/**
 * The JSON value of `expression`, where `m` is the module `lib/<file>` as
 * loaded under `userAgent`.
 */
export function evaluateOn(userAgent, file, expression) {
  const script = [
    `Object.defineProperty(globalThis, 'navigator', { value: { userAgent: ${JSON.stringify(userAgent)} }, configurable: true })`,
    `const m = await import(${JSON.stringify(new URL(file, LIB).href)})`,
    `process.stdout.write(JSON.stringify(${expression}))`,
  ].join('\n')
  const out = execFileSync(
    process.execPath,
    ['--import', REGISTER.href, '--input-type=module', '--eval', script],
    { encoding: 'utf8' },
  )
  return JSON.parse(out)
}
