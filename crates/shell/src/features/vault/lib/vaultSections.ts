export const VAULT_SECTION = 'vault'

/** Settings sections shown only while the vault is unlocked. */
export const LOCKED_SECTIONS: ReadonlySet<string> = new Set(['passwords', 'addresses', 'cards'])

/** The section a nav click scrolls to; a hidden one leads to the unlock. */
export function navTarget(id: string, vaultOpen: boolean): string {
  return !vaultOpen && LOCKED_SECTIONS.has(id) ? VAULT_SECTION : id
}

/** One labelled value in an open vault row. */
export interface VaultField {
  label: string
  value: string
}

export function revealBlock(itemHeight: number, viewHeight: number): 'nearest' | 'start' {
  return itemHeight > viewHeight ? 'start' : 'nearest'
}

/** How long after an unlock its sections still grow in as they appear. */
const GROW_WINDOW_MS = 1000
let unlockedAt = -Infinity

export function noteUnlocked(now = performance.now()): void {
  unlockedAt = now
}

export function growsIn(now = performance.now()): boolean {
  return now - unlockedAt < GROW_WINDOW_MS
}
