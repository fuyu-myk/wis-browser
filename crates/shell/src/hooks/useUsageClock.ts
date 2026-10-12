import { useQueryClient } from '@tanstack/react-query'
import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'

const TICK_MS = 60_000

/** The time, ticking each minute, while usage queries refresh as time is counted. */
export function useUsageClock(active: boolean): Date {
  const [now, setNow] = useState(() => new Date())
  const queryClient = useQueryClient()

  useEffect(() => {
    if (!active) return
    setNow(new Date())
    const timer = window.setInterval(() => setNow(new Date()), TICK_MS)
    const unlisten = listen('usage-changed', () => {
      void queryClient.invalidateQueries({ queryKey: ['usage'] })
    }).catch(() => null)
    return () => {
      window.clearInterval(timer)
      void unlisten.then((stop) => stop?.())
    }
  }, [active, queryClient])

  return now
}
