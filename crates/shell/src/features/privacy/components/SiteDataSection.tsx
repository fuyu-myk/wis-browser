import { useMemo, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import { ALL_SITE_DATA_KINDS } from '@/features/privacy/lib/clearData'
import type { WorkspaceId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { currentPlatform } from '@/lib/platform'
import { SearchIcon, XIcon } from '@/lib/icons'
import { ConfirmDialog } from '@/components/ConfirmDialog'

const SITE_DATA_KEY = ['site-data'] as const

interface SiteDataSectionProps {
  workspaceId: WorkspaceId | null
  workspaceName: string
}

/**
 * Settings → Site data: the domains that have stored cookies/cache/local
 * storage in the active workspace, with per-domain delete and a confirmed
 * clear-all.
 */
export function SiteDataSection({ workspaceId, workspaceName }: SiteDataSectionProps) {
  const qc = useQueryClient()
  const [search, setSearch] = useState('')
  const [confirmClear, setConfirmClear] = useState(false)
  const [hovered, setHovered] = useState<string | null>(null)
  const [hoveredDel, setHoveredDel] = useState<string | null>(null)

  const { data: entries = [], isLoading } = useQuery({
    queryKey: [...SITE_DATA_KEY, workspaceId],
    queryFn: () => (workspaceId ? commands.listSiteData(workspaceId) : Promise.resolve([])),
    enabled: workspaceId != null,
    staleTime: 0,
    refetchOnMount: 'always',
  })

  const refresh = () => qc.invalidateQueries({ queryKey: SITE_DATA_KEY })

  const deleteOne = useMutation({
    mutationFn: (domain: string) =>
      workspaceId ? commands.deleteSiteData(workspaceId, domain) : Promise.resolve(),
    onSuccess: refresh,
  })
  const clearAll = useMutation({
    mutationFn: () =>
      workspaceId ? commands.clearSiteData(workspaceId, ALL_SITE_DATA_KINDS) : Promise.resolve(),
    onSuccess: refresh,
  })

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return entries
    return entries.filter((e) => e.domain.toLowerCase().includes(q))
  }, [entries, search])

  if (currentPlatform() === 'linux') {
    return (
      <section id="sitedata" className="sec">
        <h2 className="sec-h">Site data</h2>
        <p className="kb-vault-msg">
          Per-site data management is not available on Linux yet.
        </p>
      </section>
    )
  }

  return (
    <section id="sitedata" className="sec">
      <h2 className="sec-h">Site data</h2>
      <div className="row">
        <div className="l">
          <span>Stored data ({workspaceName})</span>
          <span>Cookies, cache and local storage kept by sites in this workspace.</span>
        </div>
        <button
          type="button"
          className="kb-btn is-danger"
          onClick={() => setConfirmClear(true)}
          disabled={!workspaceId || entries.length === 0 || clearAll.isPending}
        >
          Clear all
        </button>
      </div>

      <div className="kb-sitedata-panel">
        <div className="kb-sitedata-search">
          <SearchIcon />
          <input
            type="text"
            placeholder="Search sites"
            aria-label="Search sites"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            spellCheck={false}
          />
        </div>

        {isLoading ? (
          <p className="kb-sitedata-empty">Loading…</p>
        ) : filtered.length === 0 ? (
          <p className="kb-sitedata-empty">
            {entries.length === 0 ? 'No stored site data.' : 'No matching sites.'}
          </p>
        ) : (
          <ul
            className="kb-sitedata-list"
            onMouseOver={(e) => {
              const target = e.target as HTMLElement
              const li = target.closest('[data-sd-domain]') as HTMLElement | null
              const domain = li?.dataset.sdDomain ?? null
              setHovered(domain)
              setHoveredDel(target.closest('.kb-sitedata-del') ? domain : null)
            }}
            onMouseLeave={() => {
              setHovered(null)
              setHoveredDel(null)
            }}
          >
            {filtered.map((entry) => (
              <li
                key={entry.domain}
                data-sd-domain={entry.domain}
                className={cx('kb-sitedata-row', { 'is-hover': hovered === entry.domain })}
              >
                <span className="kb-sitedata-domain">{entry.domain}</span>
                <button
                  type="button"
                  className={cx('kb-sitedata-del', { 'is-hot': hoveredDel === entry.domain })}
                  aria-label={`Remove data for ${entry.domain}`}
                  title={`Remove data for ${entry.domain}`}
                  onClick={() => deleteOne.mutate(entry.domain)}
                >
                  <XIcon />
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>

      <ConfirmDialog
        open={confirmClear}
        title={`Clear all site data in ${workspaceName}?`}
        message="This removes cookies, cache and local storage for every site in this workspace and can't be undone."
        confirmLabel="Clear all"
        danger
        onConfirm={() => {
          clearAll.mutate()
          setConfirmClear(false)
        }}
        onCancel={() => setConfirmClear(false)}
      />
    </section>
  )
}
