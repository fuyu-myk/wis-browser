import { useState } from 'react'

import type { Tab } from '@/bindings/types'
import { SearchIcon } from '@/lib/icons'
import { normaliseUrl } from '@/lib/url'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useActivePrivacyMode } from '@/hooks/useActivePrivacyMode'
import { useNavigateOrCreateTab } from '@/hooks/useNavigateOrCreateTab'
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

/**
 * Editorial prompts shown in the hero. One is picked at random per launch so
 * the launchpad feels alive without animation. Keep the voice consistent:
 * one capital, a single question, never cute.
 */
const LAUNCH_PROMPTS: readonly string[] = [
  'What would you like to explore today?',
  'What would you like to search today?',
  'What are you curious about?',
  'What shall we look into?',
  'What are you searching for?',
  "What's on your mind?",
  'What would you like to find?',
  'What are you looking up today?',
  "Where shall we surf to today?",
  "Where to next?",
]

function pickPrompt(): string {
  const index = Math.floor(Math.random() * LAUNCH_PROMPTS.length)
  return LAUNCH_PROMPTS[index]
}

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
  const [prompt] = useState(pickPrompt)
  const swapIcon = useWindowIconSwap()
  const mode = useActivePrivacyMode()
  const strict = mode === 'strict'

  const navigate = useNavigateOrCreateTab(activeWorkspaceId, activeTab)

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
              swapIcon(strict ? 'closed' : 'open')
            }}
          >
            <span className="kb-launch-mark">
              <WisMark
                variant={strict || logoHovered ? 'solid-closed' : 'blink'}
                size={72}
                aria-label="wis"
              />
            </span>
          </div>
          <span className="tagline">{prompt}</span>
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

