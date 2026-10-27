export const VAULT_SECTION = 'vault'

/** Settings sections shown only while the vault is unlocked. */
export const LOCKED_SECTIONS: ReadonlySet<string> = new Set(['passwords', 'addresses', 'cards'])

/** The section a nav click scrolls to; a hidden one leads to the unlock. */
export function navTarget(id: string, vaultOpen: boolean): string {
  return !vaultOpen && LOCKED_SECTIONS.has(id) ? VAULT_SECTION : id
}
