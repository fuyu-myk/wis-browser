import { useMemo, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { WorkspaceId } from '@/bindings/types'
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
    mutationFn: () => (workspaceId ? commands.clearSiteData(workspaceId) : Promise.resolve()),
    onSuccess: refresh,
  })

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase()
    if (!q) return entries
    return entries.filter((e) => e.domain.toLowerCase().includes(q))
  }, [entries, search])

  return (
    <section id="sitedata" className="sec">
      <div className="sec-h">Site data</div>
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
          <ul className="kb-sitedata-list">
            {filtered.map((entry) => (
              <li key={entry.domain} className="kb-sitedata-row">
                <span className="kb-sitedata-domain">{entry.domain}</span>
                <button
                  type="button"
                  className="kb-sitedata-del"
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
