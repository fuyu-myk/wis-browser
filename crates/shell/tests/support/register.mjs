// Loaded with `node --import`: lets Node resolve the frontend's own imports,
// which Vite resolves at build time. See `resolve.mjs`.

import { register } from 'node:module'

register('./resolve.mjs', import.meta.url)
