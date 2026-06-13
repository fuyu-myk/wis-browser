import type { Tab } from '@/bindings/types'
import { LockIcon } from '@/lib/icons'

/** Small leftward chevron on the expanded island chip: collapses to a circle. */
export function CollapseChevron() {
  return (
    <svg
      width="11"
      height="11"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="3"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      <path d="m15 18-6-6 6-6" />
    </svg>
  )
}

export function UrlContent({ tab }: { tab: Tab }) {
  const parts = (() => {
    try {
      const u = new URL(tab.url)
      if (u.protocol === 'about:' || u.host === '') {
        return null
      }
      return { domain: u.host, path: u.pathname === '/' ? '' : u.pathname }
    } catch {
      return null
    }
  })()

  return (
    <>
      <LockIcon className="ico" />
      {parts ? (
        <>
          <span className="domain">{parts.domain}</span>
          <span className="path">{parts.path}</span>
        </>
      ) : (
        <span className="path">Search or enter URL</span>
      )}
    </>
  )
}
