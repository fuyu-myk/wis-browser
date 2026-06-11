/**
 * Bundled folder/workspace glyph registry: wireframe and filled variants of
 * the same shapes (paths derived from Lucide, MIT). Kept to closed shapes
 * that read well when filled. Icon references are encoded strings validated
 * in core (`glyph:<name>` | `glyph-fill:<name>` | `emoji:<char>`).
 */

import type { ReactNode } from 'react'

export const ICON_GLYPHS: Record<string, string[]> = {
  folder: ['M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z'],
  home: ['M3 10.18a2 2 0 0 1 .71-1.53l7-5.95a2 2 0 0 1 2.58 0l7 5.95a2 2 0 0 1 .71 1.53V19a2 2 0 0 1-2 2h-4v-6a2 2 0 0 0-4 0v6H5a2 2 0 0 1-2-2Z'],
  star: ['M11.05 3.69a1 1 0 0 1 1.9 0l1.84 5.42 5.72.06a1 1 0 0 1 .59 1.8l-4.59 3.41 1.71 5.46a1 1 0 0 1-1.54 1.12L12 17.65l-4.68 3.31a1 1 0 0 1-1.54-1.12l1.71-5.46-4.59-3.41a1 1 0 0 1 .59-1.8l5.72-.06Z'],
  heart: ['M19 14c1.49-1.46 3-3.21 3-5.5A5.5 5.5 0 0 0 16.5 3c-1.76 0-3 .5-4.5 2-1.5-1.5-2.74-2-4.5-2A5.5 5.5 0 0 0 2 8.5c0 2.3 1.5 4.05 3 5.5l7 7Z'],
  bookmark: ['M6 3h12a1 1 0 0 1 1 1v17l-7-4-7 4V4a1 1 0 0 1 1-1Z'],
  bolt: ['M13 2 4.09 12.69a1 1 0 0 0 .77 1.64H11l-1 7.31 8.91-10.69a1 1 0 0 0-.77-1.64H13Z'],
  bell: ['M10.27 21a1.99 1.99 0 0 0 3.46 0M4 9a8 8 0 0 1 16 0c0 4.5 1.5 6 1.5 6h-19S4 13.5 4 9Z'],
  flag: ['M4 22V4a1 1 0 0 1 .4-.8A6 6 0 0 1 8 2c3 0 5 2 7.33 2q2 0 3.67-.8a1 1 0 0 1 1 .8v9.8a1 1 0 0 1-.4.8A6 6 0 0 1 16 16c-3 0-5-2-8-2a6 6 0 0 0-4 1.53'],
  moon: ['M20.99 14.53A8.51 8.51 0 1 1 9.47 3a9 9 0 1 0 11.52 11.53Z'],
  cloud: ['M17.5 19a4.5 4.5 0 0 0 .42-8.98 7 7 0 1 0-13.42 2.6A4.5 4.5 0 0 0 6.5 19Z'],
  leaf: ['M11 20A7 7 0 0 1 9.8 6.1C15.5 5 17 4.48 19 2c1 2 2 4.18 2 8 0 5.5-4.78 10-10 10Z', 'M2 21c0-3 1.85-5.36 5.08-6C9.5 14.52 12 13 13 12'],
  briefcase: ['M16 20V4a2 2 0 0 0-2-2h-4a2 2 0 0 0-2 2v16', 'M4 6h16a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2Z'],
  'graduation-cap': ['M21.43 9.59 12.84 5.2a2 2 0 0 0-1.66 0L2.6 9.59a1 1 0 0 0 0 1.83l8.57 4.39a2 2 0 0 0 1.66 0l8.59-4.39a1 1 0 0 0 0-1.83Z', 'M6 12.5V16a6 3 0 0 0 12 0v-3.5'],
  rocket: ['M4.5 16.5c-1.5 1.26-2 5-2 5s3.74-.5 5-2c.71-.84.7-2.13-.09-2.91a2.18 2.18 0 0 0-2.91-.09Z', 'M12 15l-3-3a22 22 0 0 1 2-3.95A12.88 12.88 0 0 1 22 2c0 2.72-.78 7.5-6 11a22.35 22.35 0 0 1-4 2Z'],
  music: ['M9 18V5l12-2v13', 'M9 18a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z', 'M21 16a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z'],
  camera: ['M20 5h-3.17L15 3H9L7.17 5H4a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2Z', 'M12 16a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7Z'],
}

export const GLYPH_NAMES = Object.keys(ICON_GLYPHS)

interface GlyphIconProps {
  name: string
  filled?: boolean
  size?: number
}

/** A registry glyph; unknown names render nothing. */
export function GlyphIcon({ name, filled = false, size = 14 }: GlyphIconProps) {
  const paths = ICON_GLYPHS[name]
  if (!paths) return null
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill={filled ? 'currentColor' : 'none'}
      stroke="currentColor"
      strokeWidth={filled ? 0 : 2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      {paths.map((d) => (
        <path key={d} d={d} />
      ))}
    </svg>
  )
}

/** Render an encoded icon reference, or null when unset/unknown. */
export function renderIconRef(icon: string | null | undefined, size = 14): ReactNode {
  if (!icon) return null
  if (icon.startsWith('glyph-fill:')) {
    return <GlyphIcon name={icon.slice('glyph-fill:'.length)} filled size={size} />
  }
  if (icon.startsWith('glyph:')) {
    return <GlyphIcon name={icon.slice('glyph:'.length)} size={size} />
  }
  if (icon.startsWith('emoji:')) {
    return (
      <span className="kb-icon-emoji" style={{ fontSize: size }} aria-hidden>
        {icon.slice('emoji:'.length)}
      </span>
    )
  }
  return null
}

/** Curated emoji choices for the picker's emoji tab. */
export const EMOJI_CHOICES = [
  '\u{1F4BC}', '\u{1F3E0}', '\u{2B50}', '\u{2764}\u{FE0F}', '\u{1F4DA}', '\u{1F3B5}',
  '\u{1F6D2}', '\u{1F3AE}', '\u{1F4F7}', '\u{2708}\u{FE0F}', '\u{1F393}', '\u{1F680}',
  '\u{1F525}', '\u{1F331}', '\u{2600}\u{FE0F}', '\u{1F319}', '\u{26A1}', '\u{1F9E0}',
  '\u{1F4B0}', '\u{1F9EA}', '\u{1F3A8}', '\u{1F4BB}', '\u{1F30D}', '\u{1F436}',
]
