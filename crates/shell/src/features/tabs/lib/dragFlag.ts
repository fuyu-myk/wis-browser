/**
 * A drag sets a flag so the click that ends it does nothing. A drag released
 * off its source fires no click, so the flag drops once the release's click
 * has had its turn instead of eating the next one.
 */
export function clearAfterClick(flag: { current: boolean }): void {
  setTimeout(() => {
    flag.current = false
  }, 0)
}
