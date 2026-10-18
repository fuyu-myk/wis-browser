/** Streamed text repaints at most 60 times a second, whatever the display's rate. */
const STREAM_FLUSH_MS = 1000 / 60
const FRAME_JITTER_MS = 2

export function flushDue(now: number, last: number): boolean {
  return now - last >= STREAM_FLUSH_MS - FRAME_JITTER_MS
}
