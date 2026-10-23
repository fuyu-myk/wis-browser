import { useState } from 'react'

import type { UsageSummary } from '@/bindings/types'
import { SiteFavicon } from '@/features/usage/components/SiteFavicon'
import { formatDuration, sharePercent } from '@/features/usage/lib/usage'

const SITE_LIMIT = 25

interface UsageSitesProps {
  sites: UsageSummary['sites']
  total: number
}

/** Sites busiest first, each with its share of `total`. */
export function UsageSites({ sites, total }: UsageSitesProps) {
  const [showAll, setShowAll] = useState(false)
  const shown = showAll ? sites : sites.slice(0, SITE_LIMIT)

  return (
    <>
      <ul className="kb-usage-sites">
        {shown.map((entry) => {
          const share = sharePercent(entry.seconds, total)
          return (
            <li key={entry.site} className="kb-usage-site">
              <span className="kb-usage-fav">
                <SiteFavicon site={entry.site} />
              </span>
              <div className="kb-usage-site-main">
                <div className="kb-usage-site-line">
                  <span className="kb-usage-site-name">{entry.site}</span>
                  <span className="kb-usage-site-time">
                    {formatDuration(entry.seconds)} · {share}%
                  </span>
                </div>
                <div className="kb-usage-share" aria-hidden="true">
                  <span style={{ width: `${share}%` }} />
                </div>
              </div>
            </li>
          )
        })}
      </ul>
      {shown.length < sites.length && (
        <button type="button" className="kb-ai-btn" onClick={() => setShowAll(true)}>
          Show all {sites.length} sites
        </button>
      )}
    </>
  )
}
