import { commands } from '@/bindings/commands'
import { isMacOS } from '@/lib/platform'

let region: HTMLElement | null = null

function liveRegion(urgent: boolean): HTMLElement {
  if (!region) {
    region = document.createElement('div')
    region.className = 'kb-sr-only'
    document.body.appendChild(region)
  }
  region.setAttribute('aria-live', urgent ? 'assertive' : 'polite')
  return region
}

/**
 * Speak `text` to a screen reader without moving focus. On macOS VoiceOver
 * is asked directly; elsewhere a persistent live region carries it.
 */
export function announce(text: string, urgent = false): void {
  if (!text.trim()) return
  if (isMacOS()) {
    void commands.announce(text, urgent).catch(() => undefined)
    return
  }
  const live = liveRegion(urgent)
  live.textContent = ''
  requestAnimationFrame(() => {
    live.textContent = text
  })
}
