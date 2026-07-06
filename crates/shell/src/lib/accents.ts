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

/** Accents cycled through when creating tab folders. Shared by the sidebar tab
 *  list and the top-bar new-folder button. */
export const FOLDER_ACCENTS: AccentColor[] = [
  'primary',
  'mint',
  'rose',
  'sun',
  'lila',
  'gray',
  'ink',
  'azur',
]

/** First accent not already in `used`, else cycle by count. */
export function nextAccent(used: AccentColor[]): AccentColor {
  for (const accent of ACCENT_ROTATION) {
    if (!used.includes(accent)) return accent
  }
  return ACCENT_ROTATION[used.length % ACCENT_ROTATION.length]
}
