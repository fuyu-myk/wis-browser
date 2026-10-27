/** One keyboard step of a split divider, in ratio units (a twentieth of 1000). */
const STEP = 50

export interface DividerLimits {
  min: number
  max: number
}

/**
 * The ratio a key moves a divider to, or null when it does not move. `row`
 * dividers (side-by-side panes) answer Left/Right, stacked ones Up/Down;
 * Home and End go to the limits.
 */
export function dividerStep(key: string, row: boolean, ratio: number, limits: DividerLimits): number | null {
  const [back, on] = row ? ['ArrowLeft', 'ArrowRight'] : ['ArrowUp', 'ArrowDown']
  const target =
    key === 'Home' ? limits.min : key === 'End' ? limits.max : key === back ? ratio - STEP : key === on ? ratio + STEP : null
  if (target === null) return null
  const clamped = Math.min(limits.max, Math.max(limits.min, target))
  return clamped === ratio ? null : clamped
}
