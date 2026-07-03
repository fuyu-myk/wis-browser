import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { ShortcutTile, Tab } from '@/bindings/types'

const FAVICON_CACHED_EVENT = 'favicon-cached'
import { SearchIcon } from '@/lib/icons'
import { cx } from '@/lib/cx'
import { normaliseUrl } from '@/lib/url'
import { reorderByKeys } from '@/lib/shortcuts'
import { useBrowserState, useUpdateSettings } from '@/hooks/useBrowserState'
import { useActivePrivacyMode } from '@/hooks/useActivePrivacyMode'
import { useNavigateOrCreateTab } from '@/hooks/useNavigateOrCreateTab'
import { usePointerReorder } from '@/hooks/usePointerReorder'
import { useWindowIconSwap } from '@/hooks/useWindowIcon'
import { useWorkspaceSlide } from '@/hooks/useWorkspaceSlide'
import { WisMark } from '@/components/WisMark'

/** DOM event the chrome dispatches when Cmd+F fires on the launchpad,
 *  asking the omnisearch input to take focus. */
export const FOCUS_LAUNCHPAD_SEARCH_EVENT = 'wis-focus-launchpad-search'

/** Deterministic fallback-tile colours, cycled by position. */
const FALLBACK_CLASSES = ['fav-mint', 'fav-gray', 'fav-ink', 'fav-rose', 'fav-azur'] as const

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

  // Launchpad tiles are drag-reorderable; the new order persists to settings.
  const update = useUpdateSettings()
  const shortcuts = state?.settings.shortcuts ?? []
  const reorder = usePointerReorder({
    keys: shortcuts.map((tile) => tile.url),
    axis: 'x',
    containerSelector: '.kb-quicklinks',
    ghostClass: 'kb-ghost-tile',
    onReorder: (keys) => update.mutate({ shortcuts: reorderByKeys(shortcuts, keys) }),
  })

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

  // Workspace switches slide the launchpad content in from the travel
  // direction, over the stationary wallpaper.
  const innerRef = useRef<HTMLDivElement>(null)
  const wsOrder = useMemo(
    () => state?.workspaces.map((w) => w.id),
    [state?.workspaces],
  )
  useWorkspaceSlide(
    'launchpad',
    state?.active_workspace_id ?? null,
    wsOrder,
    innerRef,
  )

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
      <div className="inner" ref={innerRef}>
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
          {shortcuts.slice(0, MAX_BUBBLES).map((tile, i) => (
            <QuickLinkBubble
              key={tile.url}
              tile={tile}
              index={i}
              isSource={reorder.draggedKey === tile.url}
              transform={reorder.itemTransform(i)}
              dragHandlers={reorder.dragHandlers(tile.url)}
              onActivate={(url) => {
                if (!reorder.consumeClickSuppressed()) navigate(url)
              }}
              onAuxActivate={(url) => {
                if (activeWorkspaceId) {
                  void commands
                    .createBackgroundTab(activeWorkspaceId, url)
                    .catch(() => undefined)
                }
              }}
            />
          ))}
        </div>
      </div>
    </div>
  )
}

interface QuickLinkBubbleProps {
  tile: ShortcutTile
  index: number
  /** This tile is the one being dragged (it goes invisible; the ghost moves). */
  isSource: boolean
  /** Parting transform while a sibling is dragged, or undefined. */
  transform: string | undefined
  dragHandlers: { onPointerDown: (e: React.PointerEvent) => void }
  /** Called on a real click (a press-drag is suppressed by the caller). */
  onActivate: (url: string) => void
  /** Called on a middle-click (open in a background tab). */
  onAuxActivate: (url: string) => void
}

type QuickLinkStatus = 'pending' | 'loaded' | 'failed'

function QuickLinkBubble({
  tile,
  index,
  isSource,
  transform,
  dragHandlers,
  onActivate,
  onAuxActivate,
}: QuickLinkBubbleProps) {
  const fallbackClass = FALLBACK_CLASSES[index % FALLBACK_CLASSES.length]
  const initial = (tile.label.trim()[0] ?? '?').toUpperCase()
  const [icon, setIcon] = useState<string | null>(null)
  // Three-state status so the letter fallback only appears once we know
  // the cache + prefetch round-trip has definitively failed.
  const [status, setStatus] = useState<QuickLinkStatus>('pending')
  const ownHashRef = useRef<string | null>(null)

  useEffect(() => {
    setStatus('pending')
    let cancelled = false
    const lookup = async () => {
      const hash = await commands.faviconOriginHash(tile.url).catch(() => null)
      if (cancelled) return
      if (!hash) {
        setStatus('failed')
        return
      }
      ownHashRef.current = hash
      let url = await commands.getFaviconDataUrl(hash).catch(() => null)
      if (!url) {
        const writeHash = await commands.prefetchFavicon(tile.url).catch(() => null)
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
  }, [tile.url])

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
      data-key={tile.url}
      className={cx('kb-quicklink', { 'is-dragsource': isSource })}
      style={{ transform }}
      onClick={() => onActivate(tile.url)}
      onMouseDown={(e) => {
        if (e.button === 1) e.preventDefault()
      }}
      onAuxClick={(e) => {
        if (e.button !== 1) return
        e.preventDefault()
        onAuxActivate(tile.url)
      }}
      title={tile.label}
      {...dragHandlers}
    >
      <span className="ql-tile">
        {status === 'loaded' && icon ? (
          <img
            src={icon}
            alt=""
            width={28}
            height={28}
            draggable={false}
            onError={() => setStatus('failed')}
          />
        ) : status === 'pending' ? (
          <span className="ql-pending" aria-hidden />
        ) : (
          <span className={`ql-fallback ${fallbackClass}`}>{initial}</span>
        )}
      </span>
      <span className="ql-lbl">{tile.label}</span>
    </button>
  )
}

