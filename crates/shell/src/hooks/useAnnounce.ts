import { useEffect } from 'react'

import { announce } from '@/lib/announce'

/** Speak `text` each time it appears or changes; nothing while it is empty. */
export function useAnnounce(text: string | null | false | undefined, urgent = false): void {
  useEffect(() => {
    if (text) announce(text, urgent)
  }, [text, urgent])
}
