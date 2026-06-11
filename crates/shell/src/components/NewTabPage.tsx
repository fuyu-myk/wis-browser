import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { Tab } from '@/bindings/types'

const FAVICON_CACHED_EVENT = 'favicon-cached'
import { SearchIcon } from '@/lib/icons'
import { normaliseUrl } from '@/lib/url'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useActivePrivacyMode } from '@/hooks/useActivePrivacyMode'
import { useNavigateOrCreateTab } from '@/hooks/useNavigateOrCreateTab'
import { useWindowIconSwap } from '@/hooks/useWindowIcon'
import { WisMark } from '@/components/WisMark'

/** DOM event the chrome dispatches when Cmd+F fires on the launchpad,
 *  asking the omnisearch input to take focus. */
export const FOCUS_LAUNCHPAD_SEARCH_EVENT = 'wis-focus-launchpad-search'

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
  const inputRef = useRef<HTMLInputElement | null>(null)

  const navigate = useNavigateOrCreateTab(activeWorkspaceId, activeTab)

  function submit(e: React.FormEvent) {
    e.preventDefault()
    const url = normaliseUrl(query)
    if (!url) return
    navigate(url)
    setQuery('')
  }

  useEffect(() => {
    const onFocus = () => {
      const el = inputRef.current
      if (!el) return
      el.focus()
      el.select()
    }
    window.addEventListener(FOCUS_LAUNCHPAD_SEARCH_EVENT, onFocus)
    return () => window.removeEventListener(FOCUS_LAUNCHPAD_SEARCH_EVENT, onFocus)
  }, [])

  // Shared wallpaper canvas: the backdrop is laid out once against the whole
  // content host and this pane reveals its slice.
  const rootRef = useRef<HTMLDivElement>(null)
  useLayoutEffect(() => {
    const el = rootRef.current
    if (!el) return
    const host = el.closest<HTMLElement>('.kb-content-host')
    const apply = () => {
      const h = host?.getBoundingClientRect()
      const r = el.getBoundingClientRect()
      if (!h || h.width === 0 || h.height === 0) {
        el.style.removeProperty('--wp-host-w')
        el.style.removeProperty('--wp-host-h')
        el.style.removeProperty('--wp-off-x')
        el.style.removeProperty('--wp-off-y')
        return
      }
      el.style.setProperty('--wp-host-w', `${h.width}px`)
      el.style.setProperty('--wp-host-h', `${h.height}px`)
      el.style.setProperty('--wp-off-x', `${r.left - h.left}px`)
      el.style.setProperty('--wp-off-y', `${r.top - h.top}px`)
    }
    apply()
    const ro = new ResizeObserver(apply)
    ro.observe(el)
    if (host) ro.observe(host)
    window.addEventListener('resize', apply)
    return () => {
      ro.disconnect()
      window.removeEventListener('resize', apply)
    }
  }, [])

  return (
    <div className="kb-newtab-page" ref={rootRef}>
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
            ref={inputRef}
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

type QuickLinkStatus = 'pending' | 'loaded' | 'failed'

function QuickLinkBubble({ link, onNavigate }: QuickLinkBubbleProps) {
  const [icon, setIcon] = useState<string | null>(null)
  // Three-state status so the letter fallback only appears once we know
  // the cache + prefetch round-trip has definitively failed.
  const [status, setStatus] = useState<QuickLinkStatus>('pending')
  const ownHashRef = useRef<string | null>(null)

  useEffect(() => {
    setStatus('pending')
    let cancelled = false
    const lookup = async () => {
      const hash = await commands.faviconOriginHash(link.url).catch(() => null)
      if (cancelled) return
      if (!hash) {
        setStatus('failed')
        return
      }
      ownHashRef.current = hash
      let url = await commands.getFaviconDataUrl(hash).catch(() => null)
      if (!url) {
        const writeHash = await commands.prefetchFavicon(link.url).catch(() => null)
        if (cancelled) return
        if (writeHash) {
          url = await commands.getFaviconDataUrl(writeHash).catch(() => null)
        }
      }
      if (cancelled) return
      setIcon(url)
      setStatus(url ? 'loaded' : 'failed')
    }
    void lookup()
    return () => {
      cancelled = true
    }
  }, [link.url])

  useEffect(() => {
    let unlisten: UnlistenFn | null = null
    let cancelled = false
    void listen<string>(FAVICON_CACHED_EVENT, (e) => {
      if (e.payload !== ownHashRef.current) return
      void commands
        .getFaviconDataUrl(e.payload)
        .then((url) => {
          if (cancelled) return
          setIcon(url)
          setStatus(url ? 'loaded' : 'failed')
        })
        .catch(() => undefined)
    }).then((fn) => {
      if (cancelled) fn()
      else unlisten = fn
    })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [])

  return (
    <button
      type="button"
      className="kb-quicklink"
      onClick={() => onNavigate(link.url)}
      title={link.label}
    >
      <span className="ql-tile">
        {status === 'loaded' && icon ? (
          <img
            src={icon}
            alt=""
            width={28}
            height={28}
            onError={() => setStatus('failed')}
          />
        ) : status === 'pending' ? (
          <span className="ql-pending" aria-hidden />
        ) : (
          <span className={`ql-fallback ${link.fallbackClass}`}>{link.initial}</span>
        )}
      </span>
      <span className="ql-lbl">{link.label}</span>
    </button>
  )
}

