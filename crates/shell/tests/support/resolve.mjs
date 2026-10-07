// Module resolve hook for the tests. The frontend imports through the `@/`
// alias and without file extensions, as Vite allows; Node needs both spelled
// out.

const SRC = new URL('../../src/', import.meta.url)
const EXTENSIONS = ['', '.ts', '.tsx', '/index.ts']

async function withExtensions(url, context, next) {
  let failure
  for (const extension of EXTENSIONS) {
    try {
      return await next(url + extension, context)
    } catch (err) {
      failure ??= err
    }
  }
  throw failure
}

export async function resolve(specifier, context, next) {
  if (specifier.startsWith('@/')) {
    return withExtensions(new URL(specifier.slice(2), SRC).href, context, next)
  }
  const relative = specifier.startsWith('./') || specifier.startsWith('../')
  if (relative && context.parentURL?.startsWith(SRC.href)) {
    return withExtensions(new URL(specifier, context.parentURL).href, context, next)
  }
  return next(specifier, context)
}
