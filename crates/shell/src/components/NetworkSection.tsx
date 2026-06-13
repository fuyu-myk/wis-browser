import { useEffect, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { Workspace } from '@/bindings/types'
import { Switch } from '@/components/controls'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'

type ProbeStatus =
  | { kind: 'idle' }
  | { kind: 'connecting' }
  | { kind: 'ok' }
  | { kind: 'fail'; msg: string }

export function NetworkSection({ workspace }: { workspace: Workspace | undefined }) {
  const qc = useQueryClient()
  const current = workspace?.upstream_proxy ?? null
  const on = current != null
  const upstreamKey = JSON.stringify(current ?? null)
  const [status, setStatus] = useState<ProbeStatus>({ kind: 'idle' })

  // Confirm the connection whenever the VPN is on.
  useEffect(() => {
    let cancelled = false
    if (!current) {
      setStatus({ kind: 'idle' })
      return
    }
    setStatus({ kind: 'connecting' })
    commands.testUpstream(current).then(
      () => {
        if (!cancelled) setStatus({ kind: 'ok' })
      },
      (e: unknown) => {
        if (!cancelled) {
          setStatus({ kind: 'fail', msg: e instanceof Error ? e.message : String(e) })
        }
      },
    )
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [upstreamKey])

  if (!workspace) return null
  const id = workspace.id

  const toggle = (next: boolean) => {
    if (next) setStatus({ kind: 'connecting' }) // optimistic
    void commands
      .setWorkspaceUpstream(id, next ? { kind: 'tor' } : null)
      .catch(() => undefined)
      .finally(() => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }))
  }

  return (
    <section id="network" className="sec">
      <div className="sec-h">Network</div>
      <div className="row">
        <div className="l">
          <span>VPN (Tor)</span>
          <span>Route this workspace through the built-in Tor network. First connect bootstraps and can take a few seconds.</span>
        </div>
        <Switch checked={on} onChange={toggle} label="VPN" />
      </div>
      {on && (
        <div className="row">
          <div className="l">
            <span>Connection</span>
            <span>Status of the Tor connection for this workspace.</span>
          </div>
          <ProbeBadge status={status} />
        </div>
      )}
    </section>
  )
}

function ProbeBadge({ status }: { status: ProbeStatus }) {
  if (status.kind === 'idle') return <span className="kb-net-status is-idle">Off</span>
  if (status.kind === 'connecting')
    return <span className="kb-net-status is-checking">Connecting…</span>
  if (status.kind === 'ok')
    return (
      <span className="kb-net-status is-ok">
        <span className="kb-net-dot" />
        Connected
      </span>
    )
  return (
    <span className="kb-net-status is-fail" title={status.msg}>
      <span className="kb-net-dot" />
      Can’t connect
    </span>
  )
}
