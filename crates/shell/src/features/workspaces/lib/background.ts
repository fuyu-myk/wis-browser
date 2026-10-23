/** Pure helpers for workspace backgrounds (CSS generation + defaults). */

import type { Background, GradientSpec, ImageBackground } from '@/bindings/types'

/** Hex stop colour (+ optional alpha byte) to rgba, with its alpha scaled
 * by the whole-gradient opacity factor. */
function stopColor(hex: string, factor: number): string {
  const r = parseInt(hex.slice(1, 3), 16)
  const g = parseInt(hex.slice(3, 5), 16)
  const b = parseInt(hex.slice(5, 7), 16)
  const a = hex.length === 9 ? parseInt(hex.slice(7, 9), 16) / 255 : 1
  return `rgba(${r}, ${g}, ${b}, ${(a * factor).toFixed(3)})`
}

/** CSS `linear-gradient(...)` for a gradient spec. */
export function gradientCss(spec: GradientSpec): string {
  const factor = Math.min(100, Math.max(0, spec.opacity ?? 100)) / 100
  const stops = spec.stops
    .map((s) => `${stopColor(s.color, factor)} ${s.position}%`)
    .join(', ')
  return `linear-gradient(${spec.angle}deg, ${stops})`
}

/** Layered CSS background for a workspace background: gradients render
 * directly; images layer the dim overlay over the wis-asset URL. */
export function backgroundCss(bg: Background): string | null {
  if ('gradient' in bg) return gradientCss(bg.gradient)
  const img = bg.image
  const dim = Math.min(80, Math.max(0, img.dim)) / 100
  return `linear-gradient(rgba(0, 0, 0, ${dim.toFixed(2)}), rgba(0, 0, 0, ${dim.toFixed(2)})), url("wis-asset://wallpaper/${img.asset}")`
}

/** background-size / background-repeat for an image fit. */
export function fitCss(fit: ImageBackground['fit']): { size: string; repeat: string } {
  if (fit === 'contain') return { size: 'contain', repeat: 'no-repeat' }
  if (fit === 'tile') return { size: 'auto', repeat: 'repeat' }
  return { size: 'cover', repeat: 'no-repeat' }
}

/** Seed shown when a workspace first opens the gradient editor. */
export function defaultGradient(dark: boolean): GradientSpec {
  return dark
    ? {
        angle: 160,
        opacity: 100,
        stops: [
          { color: '#1a1730', position: 0 },
          { color: '#14182e', position: 45 },
          { color: '#0f0f12', position: 100 },
        ],
      }
    : {
        angle: 160,
        opacity: 100,
        stops: [
          { color: '#efe7ff', position: 0 },
          { color: '#dde2ff', position: 40 },
          { color: '#f7f6f3', position: 100 },
        ],
      }
}
