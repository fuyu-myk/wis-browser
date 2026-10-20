import { useEffect } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { SiteProtection } from '@/bindings/types'
import { useBrowserState } from '@/hooks/useBrowserState'
import { SITE_PROTECTION_SESSION_EVENT, siteProtection } from '@/lib/sitePanel'

const SESSION_KEY = ['site-protection-session']

export function useSiteProtection(site: string | null): SiteProtection {
  const { data: state } = useBrowserState()
  const queryClient = useQueryClient()
  const { data: session } = useQuery({
    queryKey: SESSION_KEY,
    queryFn: () => commands.siteProtectionSession(),
    staleTime: Infinity,
  })

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<Record<string, SiteProtection>>(SITE_PROTECTION_SESSION_EVENT, (e) => {
      queryClient.setQueryData(SESSION_KEY, e.payload)
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [queryClient])

  return siteProtection(state?.settings.site_protection_overrides, session, site ?? '')
}
