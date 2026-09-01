import { useEffect, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { UserSettings, Workspace } from '@/bindings/types'
import { Segmented, Switch } from '@/components/controls'
import { dohProviderFor, DOH_PROVIDERS, type DohProviderId } from '@/lib/doh'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'

type ProbeStatus =
  | { kind: 'idle' }
  | { kind: 'connecting' }
  | { kind: 'ok' }
  | { kind: 'fail'; msg: string }

interface NetworkSectionProps {
  workspace: Workspace | undefined
  settings: UserSettings
}

export function NetworkSection({ workspace, settings }: NetworkSectionProps) {
  const qc = useQueryClient()
  const current = workspace?.upstream_proxy ?? null
  const on = current != null
  const upstreamKey = JSON.stringify(current ?? null)
  const [status, setStatus] = useState<ProbeStatus>({ kind: 'idle' })
  /** Result of the lookup made right after a provider change. */
  const [dns, setDns] = useState<ProbeStatus>({ kind: 'idle' })

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

  const toggle = (next: boolean) => {
    if (!workspace) return
    if (next) setStatus({ kind: 'connecting' }) // optimistic
    void commands
      .setWorkspaceUpstream(workspace.id, next ? { kind: 'tor' } : null)
      .catch(() => undefined)
      .finally(() => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }))
  }

  const provider = dohProviderFor(settings.doh_resolver)
  const dnsBypassed = on

  const chooseProvider = (id: DohProviderId) => {
    const next = DOH_PROVIDERS.find((p) => p.id === id)
    if (!next || id === provider) return
    setDns({ kind: 'connecting' })
    void commands
      .updateSettings({ doh_resolver: next.url })
      .then(() => commands.resolveHostname('example.com'))
      .then(
        () => setDns({ kind: 'ok' }),
        (e: unknown) => setDns({ kind: 'fail', msg: e instanceof Error ? e.message : String(e) }),
      )
      .finally(() => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }))
  }

  return (
    <section id="network" className="sec">
      <div className="sec-h">Network</div>
      {workspace && (
        <div className="row">
          <div className="l">
            <span>VPN (Tor)</span>
            <span>Route this workspace through the built-in Tor network. First connect bootstraps and can take a few seconds.</span>
          </div>
          <Switch checked={on} onChange={toggle} label="VPN" />
        </div>
      )}
      {workspace && on && (
        <div className="row">
          <div className="l">
            <span>Connection</span>
            <span>Status of the Tor connection for this workspace.</span>
          </div>
          <ProbeBadge status={status} />
        </div>
      )}
      <div className="row">
        <div className="l">
          <span>Encrypted DNS</span>
          <span>
            {dnsBypassed
              ? 'This workspace routes through Tor, which resolves at its exit, so this provider is not used here.'
              : DOH_PROVIDERS.find((p) => p.id === provider)?.note}
          </span>
        </div>
        <Segmented
          value={provider}
          onChange={chooseProvider}
          options={DOH_PROVIDERS.map((p) => ({ value: p.id, label: p.label }))}
          ariaLabel="Encrypted DNS provider"
        />
      </div>
      {dns.kind !== 'idle' && (
        <div className="row">
          <div className="l">
            <span>Lookup test</span>
            <span>
              {dns.kind === 'fail'
                ? 'This provider did not answer. Pages will not load until you pick one that does.'
                : 'A test lookup through the provider you picked.'}
            </span>
          </div>
          <ProbeBadge status={dns} />
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
