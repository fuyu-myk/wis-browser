import type { AccentColor } from '@/bindings/types'

/** Order new workspaces cycle through their accent colour. */
export const ACCENT_ROTATION: AccentColor[] = [
  'primary',
  'mint',
  'rose',
  'sun',
  'lila',
  'azur',
  'gray',
  'ink',
]

/** First accent not already in `used`, else cycle by count. */
export function nextAccent(used: AccentColor[]): AccentColor {
  for (const accent of ACCENT_ROTATION) {
    if (!used.includes(accent)) return accent
  }
  return ACCENT_ROTATION[used.length % ACCENT_ROTATION.length]
}
