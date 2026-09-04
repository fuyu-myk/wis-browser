/**
 * Inline checks for a typed upstream proxy, mirroring
 * `wis_core::UpstreamProxy::validate`, which stays the boundary that actually
 * refuses. These exist so the settings row can say what is wrong before the
 * round trip, not to be trusted in its place.
 */

import type { UpstreamProxy } from '@/bindings/types'

/** How a workspace reaches the network. */
export type UpstreamMode = 'off' | 'tor' | 'socks5'

export const DEFAULT_SOCKS5_PORT = 9050

export function upstreamMode(upstream: UpstreamProxy | null): UpstreamMode {
  if (upstream === null) return 'off'
  return upstream.kind === 'tor' ? 'tor' : 'socks5'
}

/** Why `host` cannot be a proxy host, or null when it can. */
export function proxyHostProblem(host: string): string | null {
  const value = host.trim()
  if (value === '') return 'Enter the proxy host.'
  // Measure in UTF-8 bytes.
  if (new TextEncoder().encode(value).length > 253) return 'That host is too long.'
  if (/[\s/@]/.test(value) || value.includes('://')) {
    return 'Enter a host and port only, with no scheme, credentials or path.'
  }
  const inner = /^\[(.*)\]$/.exec(value)?.[1]
  const bare = inner !== undefined && inner.includes(':') ? inner : value
  const bracketed = bare.includes(':') ? `[${bare}]` : bare
  try {
    if (new URL(`socks5://${bracketed}`).hostname === '') return 'That host is not valid.'
  } catch {
    return 'That host is not valid.'
  }
  return null
}

/** Why `port` cannot be a proxy port, or null when it can. */
export function proxyPortProblem(port: string): string | null {
  const value = port.trim()
  if (value === '') return 'Enter the proxy port.'
  if (!/^\d+$/.test(value)) return 'The port must be a number.'
  const parsed = Number.parseInt(value, 10)
  if (parsed < 1 || parsed > 65535) return 'The port must be between 1 and 65535.'
  return null
}
