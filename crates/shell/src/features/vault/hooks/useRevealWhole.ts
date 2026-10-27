import { useCallback, useRef } from 'react'

import { revealBlock } from '@/features/vault/lib/vaultSections'

function scrollParent(el: HTMLElement): HTMLElement | null {
  for (let node = el.parentElement; node; node = node.parentElement) {
    const { overflowY } = getComputedStyle(node)
    if ((overflowY === 'auto' || overflowY === 'scroll') && node.scrollHeight > node.clientHeight) {
      return node
    }
  }
  return null
}

/**
 * A ref that scrolls its element fully into view when it mounts and whenever
 * it grows.
 */
export function useRevealWhole<T extends HTMLElement>(): (el: T | null) => void {
  const observer = useRef<ResizeObserver | null>(null)
  return useCallback((el: T | null) => {
    observer.current?.disconnect()
    observer.current = null
    if (!el) return
    let height = 0
    observer.current = new ResizeObserver(() => {
      const next = el.offsetHeight
      if (next <= height) {
        height = next
        return
      }
      height = next
      const view = scrollParent(el)?.clientHeight ?? window.innerHeight
      el.scrollIntoView({ behavior: 'smooth', block: revealBlock(next, view) })
    })
    observer.current.observe(el)
  }, [])
}
