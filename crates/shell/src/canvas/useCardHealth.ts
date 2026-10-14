import { useEffect, useRef, useState } from 'react'

const BEAT_MS = 2000

export type CardPulse = (state: 'start' | 'beat' | 'stop') => Promise<boolean>

export interface CardHealth {
  running: boolean
  safe: boolean
  runAnyway: () => void
}

/**
 * HTML cards run only once Rust has noted the start, and report while they
 * run; Rust answers a start "safe" when the last run stopped responding.
 */
export function useCardHealth(pulse: CardPulse, wanted: boolean): CardHealth {
  const [visible, setVisible] = useState(() => document.visibilityState === 'visible')
  const [safe, setSafe] = useState(false)
  const [cleared, setCleared] = useState(false)
  const send = useRef(pulse)
  send.current = pulse
  const want = wanted && visible

  useEffect(() => {
    const onVisibility = () => setVisible(document.visibilityState === 'visible')
    document.addEventListener('visibilitychange', onVisibility)
    return () => document.removeEventListener('visibilitychange', onVisibility)
  }, [])

  useEffect(() => {
    if (!want || safe) return
    let alive = true
    let beat = 0
    const report = (state: 'beat' | 'stop') => void send.current(state).catch(() => false)
    const onHide = () => report('stop')
    window.addEventListener('pagehide', onHide)
    send.current('start').then(
      (stuck) => {
        if (stuck) setSafe(true)
        if (stuck || !alive) return
        setCleared(true)
        beat = window.setInterval(() => report('beat'), BEAT_MS)
      },
      () => alive && setCleared(true),
    )
    return () => {
      alive = false
      window.clearInterval(beat)
      window.removeEventListener('pagehide', onHide)
      report('stop')
      setCleared(false)
    }
  }, [want, safe])

  return { running: want && cleared && !safe, safe, runAnyway: () => setSafe(false) }
}
