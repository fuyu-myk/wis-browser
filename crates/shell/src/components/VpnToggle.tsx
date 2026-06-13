import { useEffect, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { Workspace } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'

interface VpnToggleProps {
  workspace: Workspace | null
}

type Phase = 'idle' | 'connecting' | 'connected' | 'error'

/**
 * One-tap VPN toggle for the active workspace, above the settings cog. "On"
 * routes the workspace through Tor (built-in Arti when compiled in, otherwise a
 * local Tor daemon); "off" is direct.
 *
 * The icon only lights up (accent) once the connection is verified via a
 * probe; while it's being set up it shows a spinner, and a failed connection is
 * shown distinctly rather than lit.
 */
export function VpnToggle({ workspace }: VpnToggleProps) {
  const qc = useQueryClient()

  const current = workspace?.upstream_proxy ?? null
  const enabled = current != null
  const upstreamKey = JSON.stringify(current)

  const [phase, setPhase] = useState<Phase>(enabled ? 'connecting' : 'idle')

  // Authoritative state: whenever the persisted upstream changes,
  // probe to confirm the connection actually works before lighting the icon.
  useEffect(() => {
    let cancelled = false
    if (!current) {
      setPhase('idle')
      return
    }
    setPhase('connecting')
    commands.testUpstream(current).then(
      () => {
        if (!cancelled) setPhase('connected')
      },
      () => {
        if (!cancelled) setPhase('error')
      },
    )
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [upstreamKey])

  if (!workspace) return null

  const onClick = () => {
    setPhase(enabled ? 'idle' : 'connecting') // optimistic; the effect reconfirms
    void commands
      .setWorkspaceUpstream(workspace.id, enabled ? null : { kind: 'tor' })
      .catch(() => undefined)
      .finally(() => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }))
  }

  const connecting = phase === 'connecting'
  const connected = phase === 'connected'
  const errored = phase === 'error'

  const label = connecting
    ? 'Setting up VPN…'
    : connected
      ? 'VPN connected — click to disconnect'
      : errored
        ? 'VPN couldn’t connect — click to turn off'
        : 'VPN off — click to connect'

  const stateClasses = { 'is-on': connected, 'is-connecting': connecting, 'is-error': errored }
  const className = cx('kb-circle-btn', 'kb-rail-vpn', stateClasses)

  return (
    <button
      type="button"
      className={className}
      aria-label={label}
      aria-pressed={connected}
      title={label}
      disabled={connecting}
      onClick={onClick}
    >
      {connecting ? <span className="kb-vpn-spinner" aria-hidden /> : <ShieldIcon on={connected} />}
    </button>
  )
}

function ShieldIcon({ on }: { on: boolean }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.9}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="M12 3 5 6v5c0 4.2 2.8 7.6 7 9 4.2-1.4 7-4.8 7-9V6l-7-3Z" />
      {on && <path d="m9 12 2 2 4-4" />}
    </svg>
  )
}
