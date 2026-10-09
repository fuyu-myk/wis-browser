/**
 * Calls `onChange` when the window resizes or `el` changes size. Resizing or
 * toggling the sidebar, or opening the assistant panel, resizes the content
 * host without resizing the window, so an overlay placed against it needs both.
 * Returns the cleanup.
 */
export function watchLayout(el: Element | null, onChange: () => void): () => void {
  window.addEventListener('resize', onChange)
  const observer =
    el && typeof ResizeObserver === 'function' ? new ResizeObserver(() => onChange()) : null
  if (el) observer?.observe(el)

  return () => {
    window.removeEventListener('resize', onChange)
    observer?.disconnect()
  }
}
