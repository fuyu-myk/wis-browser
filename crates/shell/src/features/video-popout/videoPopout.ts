// Video popout: shared event names, payload types, and the remembered
// geometry of the floating mini player. The popout itself is fully native
// (a separate always-on-top window owned by `video_popout.rs`); the chrome
// only decides when to open/close it and remembers where the user left it.

/** Per-tab media-state changes reported by the injected page script. */
export const POPOUT_MEDIA_EVENT = 'wis-popout-media'
/** The popout window opened for a tab (enter handshake completed). */
export const POPOUT_OPENED_EVENT = 'wis-popout-opened'
/** The popout closed; carries the final frame when a window was live. */
export const POPOUT_CLOSED_EVENT = 'wis-popout-closed'
/** The page's hover button asked to pop its video out. */
export const POPOUT_REQUESTED_EVENT = 'wis-popout-requested'

export interface PopoutMediaPayload {
  tab: string
  /** A qualifying video is actively playing (gates the automatic popout). */
  playing: boolean
  /** A usable video exists, playing or paused (gates the manual triggers). */
  present: boolean
}

export interface PopoutRequestedPayload {
  tab: string
}

export interface PopoutOpenedPayload {
  tab: string
}

export interface PopoutClosedPayload {
  tab: string
  x?: number
  y?: number
}

/** Remembered popout position (screen coordinates). Only the position
 *  persists: every popout starts at the predefined default size. */
export interface PopoutGeometry {
  x: number
  y: number
}

const GEOMETRY_KEY = 'wis-video-popout-geometry'

function isFiniteNumber(v: unknown): v is number {
  return typeof v === 'number' && Number.isFinite(v)
}

export function loadPopoutGeometry(): PopoutGeometry | null {
  try {
    const raw = localStorage.getItem(GEOMETRY_KEY)
    if (!raw) return null
    const parsed: unknown = JSON.parse(raw)
    if (typeof parsed !== 'object' || parsed === null) return null
    const { x, y } = parsed as Record<string, unknown>
    if (!isFiniteNumber(x) || !isFiniteNumber(y)) return null
    return { x, y }
  } catch {
    return null
  }
}

export function savePopoutGeometry(geometry: PopoutGeometry): void {
  try {
    localStorage.setItem(GEOMETRY_KEY, JSON.stringify(geometry))
  } catch {
    // Quota/private-mode failures only cost the remembered position.
  }
}
