/**
 * The encrypted-DNS providers the picker offers.
 *
 * Every connection resolves through this: the proxy has no OS-DNS fallback,
 * so the list is limited to vetted endpoints configs the resolver ships with.
 * The ids and URLs mirror `wis_network::DohProvider`, whose `doh_provider_for`
 * maps a stored URL back to a provider. A URL here that it does not recognise
 * would silently resolve through the default instead.
 */

export type DohProviderId = 'cloudflare' | 'quad9' | 'google'

export interface DohProviderOption {
  id: DohProviderId
  label: string
  /** Stored in `UserSettings.doh_resolver`. */
  url: string
  /** Shown under the row's title for the selected provider. */
  note: string
}

export const DOH_PROVIDERS: ReadonlyArray<DohProviderOption> = [
  {
    id: 'cloudflare',
    label: 'Cloudflare',
    url: 'https://cloudflare-dns.com/dns-query',
    note: 'Lookups go to Cloudflare over HTTPS. The network you are on cannot read them, but Cloudflare sees every host you visit.',
  },
  {
    id: 'quad9',
    label: 'Quad9',
    url: 'https://dns.quad9.net/dns-query',
    note: 'Lookups go to Quad9 over HTTPS. Quad9 sees every host you visit, and refuses to resolve known malicious ones.',
  },
  {
    id: 'google',
    label: 'Google',
    url: 'https://dns.google/dns-query',
    note: 'Lookups go to Google over HTTPS. Google sees every host you visit, and profiles across its other services.',
  },
]

/** The provider a stored URL resolves through, mirroring `doh_provider_for`. */
export function dohProviderFor(url: string | null): DohProviderId {
  if (url === null) return 'cloudflare'
  let host: string
  try {
    host = new URL(url).hostname.toLowerCase()
  } catch {
    return 'cloudflare'
  }
  if (host === 'dns.quad9.net' || host === '9.9.9.9' || host === '149.112.112.112') return 'quad9'
  if (host === 'dns.google' || host === '8.8.8.8' || host === '8.8.4.4') return 'google'
  return 'cloudflare'
}
