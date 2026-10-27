import type { MotionLevel } from '@/bindings/types'

/**
 * Motion tiers, set as body classes by `useThemeApply`. Full moves; Reduced
 * keeps fades and colour changes but nothing slides, grows or springs; Off
 * stops everything.
 */
interface MotionHost {
  classList: { contains(name: string): boolean } | { has(name: string): boolean }
}

function has(host: MotionHost, name: string): boolean {
  const list = host.classList
  return 'contains' in list ? list.contains(name) : list.has(name)
}

/** The tier in effect: the OS reduce-motion preference caps Full at Reduced. */
export function effectiveMotion(setting: MotionLevel, osReduce: boolean): MotionLevel {
  return setting === 'full' && osReduce ? 'reduced' : setting
}

export function motionOff(host: MotionHost = document.body): boolean {
  return has(host, 'motion-off')
}

/** Nothing may move: Reduced or Off. Fades may still play on Reduced. */
export function motionStill(host: MotionHost = document.body): boolean {
  return has(host, 'motion-off') || has(host, 'motion-reduced')
}

export function scrollBehavior(host: MotionHost = document.body): ScrollBehavior {
  return motionStill(host) ? 'auto' : 'smooth'
}
