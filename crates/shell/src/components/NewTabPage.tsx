import { useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { Tab, WorkspaceId } from '@/bindings/types'
import { SearchIcon } from '@/lib/icons'
import { NEW_TAB_URL, normaliseUrl } from '@/lib/url'
import { BROWSER_STATE_KEY, useBrowserState } from '@/hooks/useBrowserState'
import { useWindowIconSwap } from '@/hooks/useWindowIcon'
import { WisMark } from '@/components/WisMark'

interface QuickLink {
  label: string
  url: string
  fallbackClass: string
  initial: string
}

const QUICK_LINKS: QuickLink[] = [
  { label: 'DuckDuckGo', url: 'https://duckduckgo.com/', fallbackClass: 'fav-mint', initial: 'D' },
  { label: 'Wikipedia', url: 'https://wikipedia.org/', fallbackClass: 'fav-gray', initial: 'W' },
  { label: 'GitHub', url: 'https://github.com/', fallbackClass: 'fav-ink', initial: 'G' },
  { label: 'YouTube', url: 'https://youtube.com/', fallbackClass: 'fav-rose', initial: 'Y' },
  { label: 'Hacker News', url: 'https://news.ycombinator.com/', fallbackClass: 'fav-azur', initial: 'H' },
]

const MAX_BUBBLES = 8

function faviconFor(url: string): string {
  try {
    const host = new URL(url).hostname
    return `https://www.google.com/s2/favicons?domain=${host}&sz=128`
  } catch {
    return ''
  }
}

interface NewTabPageProps {
  /** The tab currently showing this page. */
  activeTab: Tab | null
}

export function NewTabPage({ activeTab }: NewTabPageProps) {
  const { data: state } = useBrowserState()
  const activeWorkspaceId = state?.active_workspace_id ?? null
  const [query, setQuery] = useState('')
  const [logoHovered, setLogoHovered] = useState(false)
  const swapIcon = useWindowIconSwap()

  const navigate = useNavigateOrCreate(activeWorkspaceId, activeTab)

  function submit(e: React.FormEvent) {
    e.preventDefault()
    const url = normaliseUrl(query)
    if (!url) return
    navigate(url)
    setQuery('')
  }

  return (
    <div className="kb-newtab-page">
      <div className="inner">
        <div className="kb-hero">
          <div
            className="brand-row"
            onMouseEnter={() => {
              setLogoHovered(true)
              swapIcon('closed')
            }}
            onMouseLeave={() => {
              setLogoHovered(false)
              swapIcon('open')
            }}
          >
            <span className="kb-launch-mark" aria-hidden="true">
              <WisMark
                variant={logoHovered ? 'solid-closed' : 'blink'}
                size={72}
                aria-label="wis"
              />
            </span>
            <span className="wordmark">wis</span>
          </div>
          <span className="tagline">What would you like to explore today?</span>
        </div>
        <form className="kb-omnisearch" onSubmit={submit}>
          <SearchIcon />
          <input
            type="text"
            placeholder="Enter search or web address"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            autoFocus
            spellCheck={false}
          />
        </form>
        <div className="kb-quicklinks">
          {QUICK_LINKS.slice(0, MAX_BUBBLES).map((link) => (
            <QuickLinkBubble key={link.label} link={link} onNavigate={navigate} />
          ))}
        </div>
      </div>
    </div>
  )
}

interface QuickLinkBubbleProps {
  link: QuickLink
  onNavigate: (url: string) => void
}

function QuickLinkBubble({ link, onNavigate }: QuickLinkBubbleProps) {
  const [iconFailed, setIconFailed] = useState(false)
  const icon = faviconFor(link.url)

  return (
    <button
      type="button"
      className="kb-quicklink"
      onClick={() => onNavigate(link.url)}
      title={link.label}
    >
      <span className="ql-tile">
        {icon && !iconFailed ? (
          <img
            src={icon}
            alt=""
            width={28}
            height={28}
            onError={() => setIconFailed(true)}
          />
        ) : (
          <span className={`ql-fallback ${link.fallbackClass}`}>{link.initial}</span>
        )}
      </span>
      <span className="ql-lbl">{link.label}</span>
    </button>
  )
}

/**
 * If there's an `about:newtab` placeholder active, navigate it. Otherwise
 * create a new tab at the URL in the active workspace.
 */
function useNavigateOrCreate(
  workspaceId: WorkspaceId | null,
  placeholderTab: Tab | null,
): (url: string) => void {
  const qc = useQueryClient()
  const navigateMutation = useMutation({
    mutationFn: ({ id, url }: { id: Tab['id']; url: string }) =>
      commands.navigateTab(id, url),
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })
  const createMutation = useMutation({
    mutationFn: ({ workspaceId, url }: { workspaceId: WorkspaceId; url: string }) =>
      commands.createTab(workspaceId, url),
    onSuccess: () => qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY }),
  })

  return (url) => {
    if (placeholderTab && placeholderTab.url === NEW_TAB_URL) {
      navigateMutation.mutate({ id: placeholderTab.id, url })
    } else if (workspaceId) {
      createMutation.mutate({ workspaceId, url })
    }
  }
}
