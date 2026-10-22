import { useMemo, useState } from 'react'

import type { UserSettingsPatch, WorkspaceId } from '@/bindings/types'
import { useBrowserState } from '@/hooks/useBrowserState'
import { looksLikeSite, MAX_LINK_ROUTES, withoutRoute, withRoute } from '@/lib/linkRoutes'

interface LinkRoutesProps {
  onUpdate: (patch: UserSettingsPatch) => void
}

export function LinkRoutes({ onUpdate }: LinkRoutesProps) {
  const { data: state } = useBrowserState()
  const workspaces = useMemo(
    () => state?.workspaces.filter((w) => !w.ephemeral) ?? [],
    [state?.workspaces],
  )
  const routes = useMemo(() => state?.settings.link_routes ?? [], [state?.settings.link_routes])
  const shown = useMemo(
    () => [...routes].sort((a, b) => a.pattern.localeCompare(b.pattern)),
    [routes],
  )
  const [site, setSite] = useState('')
  const [chosen, setChosen] = useState<WorkspaceId | null>(null)
  const target = workspaces.some((w) => w.id === chosen) ? chosen : (workspaces[0]?.id ?? null)
  const canAdd =
    target !== null &&
    looksLikeSite(site) &&
    withRoute(routes, site, target).length <= MAX_LINK_ROUTES

  const add = () => {
    if (!canAdd) return
    onUpdate({ link_routes: withRoute(routes, site, target) })
    setSite('')
  }
  const nameOf = (id: WorkspaceId) => workspaces.find((w) => w.id === id)?.name ?? 'Workspace'

  return (
    <>
      <div className="row">
        <div className="l">
          <span>Links from other apps</span>
          <span>
            Open these sites in a workspace of their own. The most specific site wins;
            other links open in the workspace you are in.
          </span>
        </div>
      </div>
      <div className="kb-ua-add">
        <input
          type="text"
          className="kb-text-input"
          placeholder="figma.com or github.com/acme"
          value={site}
          onChange={(e) => setSite(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') add()
          }}
          aria-label="Site"
          spellCheck={false}
        />
        <span className="kb-select">
          <select
            value={target ?? ''}
            onChange={(e) => setChosen(e.currentTarget.value as WorkspaceId)}
            aria-label="Workspace for the site"
          >
            {workspaces.map((w) => (
              <option key={w.id} value={w.id}>
                {w.name}
              </option>
            ))}
          </select>
        </span>
        <button type="button" className="kb-btn" onClick={add} disabled={!canAdd}>
          Add
        </button>
      </div>
      {shown.length > 0 && (
        <ul className="kb-ua-list">
          {shown.map((route) => (
            <li key={route.pattern} className="kb-ua-item">
              <span className="kb-ua-host">{route.pattern}</span>
              <span className="kb-ua-prof">{nameOf(route.workspace_id)}</span>
              <button
                type="button"
                className="kb-ua-remove"
                onClick={() => onUpdate({ link_routes: withoutRoute(routes, route.pattern) })}
                aria-label={`Remove the rule for ${route.pattern}`}
              >
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}
    </>
  )
}
