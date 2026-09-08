/**
 * Keeps a selection visible while the page menu is open.
 *
 * Opening the menu moves focus to its own webview, and an unfocused document
 * stops painting its selection. The highlight API draws over the same ranges
 * without touching the DOM.
 *
 * This is the chrome's copy. The page has its own inside `page_menu.rs`.
 */

const HIGHLIGHT = 'wis-menu-selection'
const STYLE_ID = 'wis-menu-selection-style'

/** Rises per paint, so a deferred drop cannot take a newer one with it. */
let paintGen = 0

/** Longest selection carried out of the chrome, matching the page script. */
export const MAX_SELECTION_CHARS = 32000

interface HighlightRegistry {
  set: (name: string, highlight: unknown) => void
  delete: (name: string) => void
}

type HighlightCtor = new () => { add: (range: Range) => void; size: number }

function registry(): HighlightRegistry | null {
  const highlights = (CSS as unknown as { highlights?: HighlightRegistry }).highlights
  return highlights ?? null
}

function rule(declarations: string): string {
  return `::highlight(${HIGHLIGHT}){${declarations}}`
}

/**
 * `Highlight` already tracks the document's colour scheme and carries the
 * engine's own alpha, so the chrome needs no branch of its own.
 */
const PAINT = rule('background-color:Highlight')

function styleNode(): HTMLStyleElement {
  const found = document.getElementById(STYLE_ID)
  if (found instanceof HTMLStyleElement) return found

  const style = document.createElement('style')
  style.id = STYLE_ID
  document.head.appendChild(style)
  return style
}

/** Draw over `selection`'s ranges so they survive the focus change. */
export function paintSelection(selection: Selection): void {
  const highlights = registry()
  const ctor = (window as unknown as { Highlight?: HighlightCtor }).Highlight
  if (!highlights || !ctor) return

  try {
    const painted = new ctor()
    for (let i = 0; i < selection.rangeCount; i += 1) {
      const range = selection.getRangeAt(i)
      if (!range.collapsed) painted.add(range.cloneRange())
    }
    if (painted.size === 0) return

    paintGen += 1
    styleNode().textContent = PAINT
    highlights.set(HIGHLIGHT, painted)
  } catch {
    // A browser without the API simply keeps the old behaviour.
  }
}

/** Release the drawing. */
export function clearSelectionPaint(): void {
  try {
    if (!registry()) return
    if (document.getElementById(STYLE_ID)) {
      styleNode().textContent = rule('background-color:transparent')
    }
    const gen = paintGen
    requestAnimationFrame(() => {
      if (gen !== paintGen) return
      try {
        registry()?.delete(HIGHLIGHT)
      } catch {
        // The document went away first.
      }
    })
  } catch {
    // Nothing was painted.
  }
}
