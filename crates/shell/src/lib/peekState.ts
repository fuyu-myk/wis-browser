// Whether this window's Peek overlay is currently open. Mirrors
// usePeekOverlay's internal state at module level.
// Each window's chrome webview tracks only its own peek.

let peekOpen = false

export function setPeekOpen(open: boolean): void {
  peekOpen = open
}

export function isPeekOpen(): boolean {
  return peekOpen
}
