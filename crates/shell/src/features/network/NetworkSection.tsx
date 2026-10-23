import { useEffect, useId, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import { cx } from '@/lib/cx'
import type { UserSettings, Workspace } from '@/bindings/types'
import { Segmented, Switch } from '@/components/controls'
import { dohProviderFor, DOH_PROVIDERS, type DohProviderId } from '@/features/network/doh'
import { isFeatureUnavailableHere } from '@/lib/platformSupport'
import {
  DEFAULT_SOCKS5_PORT,
  proxyHostProblem,
  proxyPortProblem,
  upstreamMode,
  type UpstreamMode,
} from '@/features/network/upstream'
import { BROWSER_STATE_KEY } from '@/hooks/useBrowserState'

/** How often an active upstream is re-checked while this page is open. */
const PROBE_INTERVAL_MS = 30_000

const ROUTE_OPTIONS = [
  { value: 'off' as const, label: 'Direct' },
  { value: 'tor' as const, label: 'Tor' },
  { value: 'socks5' as const, label: 'SOCKS5' },
] satisfies ReadonlyArray<{ value: UpstreamMode; label: string }>

function errorMessage(error: unknown): string {
  if (typeof error === 'string') return error
  if (error instanceof Error) return error.message
  if (
    error !== null &&
    typeof error === 'object' &&
    'message' in error &&
    typeof (error as { message: unknown }).message === 'string'
  ) {
    return (error as { message: string }).message
  }
  return 'unknown error'
}

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
  const mode = upstreamMode(current)
  /** Shows the host/port fields before anything is stored. */
  const [manualOpen, setManualOpen] = useState(mode === 'socks5')
  const [hostDraft, setHostDraft] = useState(
    current?.kind === 'socks5' ? current.host : '127.0.0.1',
  )
  const [portDraft, setPortDraft] = useState(
    String(current?.kind === 'socks5' ? current.port : DEFAULT_SOCKS5_PORT),
  )
  const [torAvailable, setTorAvailable] = useState(true)
  /** A refused route, kept apart from the probe so it is not overwritten. */
  const [applyError, setApplyError] = useState<string | null>(null)
  const problemId = useId()

  useEffect(() => {
    void commands.torAvailable().then(setTorAvailable, () => setTorAvailable(false))
  }, [])

  useEffect(() => {
    const carried = workspace?.upstream_proxy ?? null
    setManualOpen(upstreamMode(carried) === 'socks5')
    setHostDraft(carried?.kind === 'socks5' ? carried.host : '127.0.0.1')
    setPortDraft(String(carried?.kind === 'socks5' ? carried.port : DEFAULT_SOCKS5_PORT))
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workspace?.id])

  // Confirm the connection whenever the VPN is on.
  useEffect(() => {
    let cancelled = false
    if (!current) {
      setStatus({ kind: 'idle' })
      return
    }
    setStatus({ kind: 'connecting' })
    // A slow or bootstrapping upstream can outlast the interval; without this
    // the attempts would stack up behind each other.
    let inFlight = false
    const probe = () => {
      if (inFlight) return Promise.resolve()
      inFlight = true
      return commands.testUpstream(current).then(
        () => {
          if (!cancelled) setStatus({ kind: 'ok' })
        },
        (e: unknown) => {
          if (!cancelled) {
            setStatus({ kind: 'fail', msg: errorMessage(e) })
          }
        },
      )
      .finally(() => {
        inFlight = false
      })
    }
    void probe()
    const timer = window.setInterval(() => void probe(), PROBE_INTERVAL_MS)
    return () => {
      cancelled = true
      window.clearInterval(timer)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [upstreamKey])

  const [lockdownError, setLockdownError] = useState<string | null>(null)
  const setLockdown = (enabled: boolean) => {
    if (!workspace) return
    setLockdownError(null)
    void commands
      .setWorkspaceLockdown(workspace.id, enabled)
      .catch((e: unknown) => setLockdownError(errorMessage(e)))
      .finally(() => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }))
  }

  const apply = (next: Workspace['upstream_proxy']) => {
    if (!workspace) return
    if (JSON.stringify(next ?? null) === upstreamKey) return
    setApplyError(null)
    if (next !== null) setStatus({ kind: 'connecting' }) // optimistic
    void commands
      .setWorkspaceUpstream(workspace.id, next)
      .then(
        () => {
          if (next === null) setStatus({ kind: 'idle' })
        },
        (e: unknown) => {
          setApplyError(errorMessage(e))
          setStatus({ kind: 'idle' })
        },
      )
      .finally(() => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }))
  }

  const chooseRoute = (next: UpstreamMode) => {
    setManualOpen(next === 'socks5')
    if (next === 'socks5') return
    apply(next === 'tor' ? { kind: 'tor' } : null)
  }
  const showManual = manualOpen || mode === 'socks5'
  /** SOCKS5 is selected but nothing routes through it yet. */
  const unapplied = showManual && mode !== 'socks5'

  const hostProblem = proxyHostProblem(hostDraft)
  const portProblem = proxyPortProblem(portDraft)
  const manualProblem = hostProblem ?? portProblem
  const applyManual = () => {
    if (manualProblem !== null) return
    apply({ kind: 'socks5', host: hostDraft.trim(), port: Number.parseInt(portDraft, 10) })
  }

  const provider = dohProviderFor(settings.doh_resolver)
  // Both upstreams resolve for themselves.
  const dnsNote =
    mode === 'tor'
      ? 'This workspace routes through Tor, which resolves at its exit, so this provider is not used here.'
      : mode === 'socks5'
        ? 'This workspace routes through a SOCKS5 proxy, which resolves for itself, so this provider is not used here.'
        : DOH_PROVIDERS.find((p) => p.id === provider)?.note

  const chooseProvider = (id: DohProviderId) => {
    const next = DOH_PROVIDERS.find((p) => p.id === id)
    if (!next || id === provider) return
    setDns({ kind: 'connecting' })
    void commands
      .updateSettings({ doh_resolver: next.url })
      .then(() => commands.resolveHostname('example.com'))
      .then(
        () => setDns({ kind: 'ok' }),
        (e: unknown) => setDns({ kind: 'fail', msg: errorMessage(e) }),
      )
      .finally(() => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }))
  }

  return (
    <section id="network" className="sec">
      <div className="sec-h">Network</div>
      {workspace && (
        <div className="row">
          <div className="l">
            <span>Route this workspace</span>
            <span className={cx({ 'is-error': applyError !== null })}>
              {applyError !== null
                ? applyError
                : unapplied
                ? `Not applied yet: this workspace still routes ${
                    mode === 'tor' ? 'through Tor' : 'directly'
                  }.`
                : mode === 'tor'
                  ? 'Through the built-in Tor network. The first connection bootstraps and can take a few seconds. Lockdown Mode below hardens it further.'
                  : mode === 'socks5'
                    ? 'Through the SOCKS5 proxy below, which resolves its own DNS.'
                    : torAvailable
                      ? 'Straight out, with no proxy in between.'
                      : 'Straight out. Built-in Tor is not included in this build.'}
            </span>
          </div>
          <Segmented
            value={showManual ? 'socks5' : mode}
            onChange={chooseRoute}
            options={ROUTE_OPTIONS.filter((o) => o.value !== 'tor' || torAvailable)}
            ariaLabel="Upstream route"
          />
        </div>
      )}
      {workspace && showManual && (
        <div className="row">
          <div className="l">
            <span>SOCKS5 proxy</span>
            <span
              id={problemId}
              aria-live="polite"
              className={cx({ 'is-error': manualProblem !== null })}
            >
              {manualProblem ?? 'The host and port of a proxy running on your machine or network.'}
            </span>
          </div>
          <div className="kb-shortcut-add">
            <input
              type="text"
              className="kb-text-input"
              placeholder="127.0.0.1"
              value={hostDraft}
              aria-invalid={hostProblem !== null}
              aria-describedby={problemId}
              onChange={(e) => setHostDraft(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') applyManual()
              }}
              aria-label="SOCKS5 proxy host"
            />
            <input
              type="text"
              inputMode="numeric"
              className="kb-text-input kb-port-input"
              placeholder={String(DEFAULT_SOCKS5_PORT)}
              value={portDraft}
              aria-invalid={portProblem !== null}
              aria-describedby={problemId}
              onChange={(e) => setPortDraft(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') applyManual()
              }}
              aria-label="SOCKS5 proxy port"
            />
            <button
              type="button"
              className="kb-btn is-primary"
              onClick={applyManual}
              disabled={manualProblem !== null}
              aria-describedby={problemId}
            >
              Apply
            </button>
          </div>
        </div>
      )}
      {workspace && on && (
        <div className="row">
          <div className="l">
            <span>Connection</span>
            <span className={cx({ 'is-error': status.kind === 'fail' })}>
              {status.kind === 'fail'
                ? status.msg
                : mode === 'tor'
                  ? 'Status of the Tor connection for this workspace.'
                  : 'Whether the proxy answers and speaks SOCKS5.'}
            </span>
          </div>
          <ProbeBadge status={status} />
        </div>
      )}
      {workspace && !isFeatureUnavailableHere('lockdown_mode') && (
        <div className="row">
          <div className="l">
            <span>Lockdown Mode</span>
            <span className={cx({ 'is-error': lockdownError !== null })}>
              {lockdownError ??
                'Turns off what attacks most often exploit: the JavaScript compiler, WebAssembly, WebGL, WebRTC, service workers, and camera and microphone access. Some sites break. Open tabs reload.'}
            </span>
          </div>
          <Switch checked={workspace.lockdown} onChange={setLockdown} label="Lockdown Mode" />
        </div>
      )}
      <div className="row">
        <div className="l">
          <span>Encrypted DNS</span>
          <span>
            {dnsNote}
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
