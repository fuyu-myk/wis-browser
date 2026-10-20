import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react'

import type { TabId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { useBrowserState, useUpdateSettings } from '@/hooks/useBrowserState'
import { useScrollMemory } from '@/hooks/useScrollMemory'
import { DownloadIcon, GlobeIcon, LockIcon, MonitorIcon, PanelTopIcon, PlusIcon, SearchIcon, SunIcon } from '@/lib/icons'
import {
  BookmarkIcon,
  DatabaseIcon,
  GridIcon,
  ImportIcon,
  KeyboardIcon,
  KeyIcon,
  PaletteIcon,
  PuzzleIcon,
  ShieldIcon,
  SparkleIcon,
  WifiIcon,
} from '@/components/settingsIcons'
import { BookmarksSection } from '@/components/BookmarksSection'
import { ImportSection } from '@/components/ImportSection'
import { ExtensionsSection } from '@/components/ExtensionsSection'
import { SearchSection } from '@/components/SearchSection'
import { AppearanceSection } from '@/components/AppearanceSection'
import { DesignSection } from '@/components/DesignSection'
import { LayoutSection } from '@/components/LayoutSection'
import { NewTabSection } from '@/components/NewTabSection'
import { SidebarWidgetsSection } from '@/components/SidebarWidgetsSection'
import { PerformanceSection } from '@/components/PerformanceSection'
import { DownloadsSection } from '@/components/DownloadsSection'
import { PrivacySection } from '@/components/PrivacySection'
import { NetworkSection } from '@/components/NetworkSection'
import { CompatibilitySection } from '@/components/CompatibilitySection'
import { SiteDataSection } from '@/components/SiteDataSection'
import { PasswordsSection } from '@/components/PasswordsSection'
import { KeyboardShortcutsSection } from '@/components/KeyboardShortcutsSection'
import { BackupSection } from '@/components/BackupSection'
import { AiSection } from '@/components/AiSection'
import { SettingsSearch } from '@/components/SettingsSearch'

interface NavSection {
  id: string
  label: string
  icon: React.ReactNode
}

const NAV_SECTIONS: ReadonlyArray<NavSection> = [
  { id: 'appearance', label: 'Appearance', icon: <SunIcon /> },
  { id: 'design', label: 'Design', icon: <PaletteIcon /> },
  { id: 'layout', label: 'Layout', icon: <PanelTopIcon /> },
  { id: 'search', label: 'Search', icon: <SearchIcon /> },
  { id: 'newtab', label: 'New tab', icon: <PlusIcon /> },
  { id: 'widgets', label: 'Sidebar widgets', icon: <GridIcon /> },
  { id: 'performance', label: 'Performance', icon: <MonitorIcon /> },
  { id: 'downloads', label: 'Downloads', icon: <DownloadIcon /> },
  { id: 'privacy', label: 'Privacy', icon: <ShieldIcon /> },
  { id: 'network', label: 'Network', icon: <WifiIcon /> },
  { id: 'compatibility', label: 'Compatibility', icon: <GlobeIcon /> },
  { id: 'sitedata', label: 'Site data', icon: <DatabaseIcon /> },
  { id: 'bookmarks', label: 'Bookmarks', icon: <BookmarkIcon /> },
  { id: 'import', label: 'Import', icon: <ImportIcon /> },
  { id: 'extensions', label: 'Extensions', icon: <PuzzleIcon /> },
  { id: 'passwords', label: 'Passwords', icon: <KeyIcon /> },
  { id: 'ai', label: 'AI assistant', icon: <SparkleIcon /> },
  { id: 'shortcuts', label: 'Shortcuts', icon: <KeyboardIcon /> },
  { id: 'backup', label: 'Backup & restore', icon: <LockIcon /> },
]

export function SettingsPage({ tabId }: { tabId: TabId }) {
  const { data: state } = useBrowserState()
  const update = useUpdateSettings()
  const activeWorkspace = state?.workspaces.find((w) => w.id === state.active_workspace_id)

  const mainRef = useRef<HTMLDivElement | null>(null)
  const attachScroll = useScrollMemory(tabId)
  const setMain = useCallback(
    (el: HTMLDivElement | null) => {
      mainRef.current = el
      attachScroll(el)
    },
    [attachScroll],
  )
  const mainEl = useCallback(() => mainRef.current, [])
  const [query, setQuery] = useState('')
  const searching = query !== ''
  const [activeSection, setActiveSection] = useState<string>(NAV_SECTIONS[0].id)
  // Non-null while a click-initiated smooth scroll is in flight.
  const scrollLockRef = useRef<number | null>(null)
  const navRef = useRef<HTMLElement | null>(null)
  const [navInd, setNavInd] = useState<{
    top: number
    height: number
    left: number
    width: number
  } | null>(null)
  useLayoutEffect(() => {
    const btn = navRef.current?.querySelector<HTMLElement>('button.is-active')
    if (!btn) {
      setNavInd(null)
      return
    }
    setNavInd({
      top: btn.offsetTop,
      height: btn.offsetHeight,
      left: btn.offsetLeft,
      width: btn.offsetWidth,
    })
  }, [activeSection, searching])

  // Highlight the section currently dominating the viewport.
  useEffect(() => {
    const root = mainRef.current
    if (!root) return
    const observed = NAV_SECTIONS.map((s) => root.querySelector<HTMLElement>(`#${s.id}`)).filter(
      (el): el is HTMLElement => el !== null,
    )
    if (observed.length === 0) return
    const obs = new IntersectionObserver(
      (entries) => {
        if (scrollLockRef.current !== null) return
        const visible = entries
          .filter((e) => e.isIntersecting)
          .sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top)
        if (visible.length > 0 && visible[0].target.id) {
          setActiveSection(visible[0].target.id)
        }
      },
      { root, threshold: 0, rootMargin: '0px 0px -70% 0px' },
    )
    for (const el of observed) obs.observe(el)
    return () => obs.disconnect()
  }, [])

  if (!state) return null
  const settings = state.settings
  const navOnRight =
    settings.layout === 'sidebar_right' ||
    (settings.layout === 'top_tabs' && settings.top_tabs_rail_side === 'right')

  // Optimistic: the indicator moves before the smooth scroll lands.
  const showSection = (id: string) => {
    setActiveSection(id)
    if (scrollLockRef.current !== null) window.clearTimeout(scrollLockRef.current)
    scrollLockRef.current = window.setTimeout(() => {
      scrollLockRef.current = null
    }, 700)
  }

  const scrollTo = (id: string) => {
    showSection(id)
    const el = mainRef.current?.querySelector(`#${id}`)
    if (el) el.scrollIntoView({ behavior: 'smooth', block: 'start' })
  }

  const reveal = (row: HTMLElement) => {
    const section = row.closest<HTMLElement>('.sec')?.id
    if (section) showSection(section)
    row.scrollIntoView({ behavior: 'smooth', block: 'center' })
    row.classList.remove('is-found')
    void row.offsetWidth
    row.classList.add('is-found')
    row.addEventListener('animationend', () => row.classList.remove('is-found'), { once: true })
  }

  return (
    <div
      className={cx('kb-settings-page', { 'nav-right': navOnRight })}
      aria-label="Settings"
      onFocus={(e) => {
        const t = e.target
        if (t instanceof HTMLInputElement && (t.type === 'number' || t.type === 'text')) {
          t.select()
          t.dataset.selectAll = '1'
        }
      }}
      onMouseUp={(e) => {
        const t = e.target
        if (t instanceof HTMLInputElement && t.dataset.selectAll === '1') {
          delete t.dataset.selectAll
          e.preventDefault()
        }
      }}
    >
      <aside className="kb-settings-nav" aria-label="Settings sections" ref={navRef}>
        <div className="kb-settings-nav-title">Settings</div>
        <SettingsSearch query={query} onQuery={setQuery} root={mainEl} onPick={reveal} />
        {!searching && navInd !== null && (
          <span
            className="kb-settings-nav-ind"
            style={{
              top: navInd.top,
              height: navInd.height,
              left: navInd.left,
              width: navInd.width,
            }}
            aria-hidden
          />
        )}
        {!searching &&
          NAV_SECTIONS.map((s) => (
            <button
              key={s.id}
              type="button"
              className={cx({ 'is-active': activeSection === s.id })}
              onClick={() => scrollTo(s.id)}
            >
              {s.icon}
              <span>{s.label}</span>
            </button>
          ))}
      </aside>
      <div className="kb-settings-main" ref={setMain}>
        <AppearanceSection
          settings={settings}
          activeWorkspace={activeWorkspace}
          onUpdate={(patch) => update.mutate(patch)}
        />
        <DesignSection settings={settings} onUpdate={(patch) => update.mutate(patch)} />
        <LayoutSection settings={settings} onUpdate={(patch) => update.mutate(patch)} />
        <SearchSection settings={settings} onUpdate={(patch) => update.mutate(patch)} />
        <NewTabSection settings={settings} onUpdate={(patch) => update.mutate(patch)} />
        <SidebarWidgetsSection
          widgets={settings.sidebar_widgets}
          onUpdate={(patch) => update.mutate(patch)}
        />
        <PerformanceSection settings={settings} onUpdate={(patch) => update.mutate(patch)} />
        <DownloadsSection settings={settings} onUpdate={(patch) => update.mutate(patch)} />
        <PrivacySection
          settings={settings}
          activeWorkspace={activeWorkspace}
          onUpdate={(patch) => update.mutate(patch)}
        />
        <NetworkSection workspace={activeWorkspace} settings={settings} />
        <CompatibilitySection
          profile={settings.user_agent_profile}
          overrides={settings.ua_site_overrides}
          onUpdate={(patch) => update.mutate(patch)}
        />
        <SiteDataSection
          workspaceId={activeWorkspace?.id ?? null}
          workspaceName={activeWorkspace?.name ?? 'workspace'}
        />
        <BookmarksSection
          workspaceId={activeWorkspace?.id ?? null}
          ephemeral={activeWorkspace?.ephemeral ?? false}
        />
        <ImportSection
          workspaceId={activeWorkspace?.id ?? null}
          ephemeral={activeWorkspace?.ephemeral ?? false}
        />
        <ExtensionsSection
          installed={settings.extensions}
          workspace={activeWorkspace}
          storeSearch={settings.extension_store_search}
          onUpdate={(patch) => update.mutate(patch)}
        />
        <PasswordsSection />
        <AiSection settings={settings} onUpdate={(patch) => update.mutate(patch)} />
        <KeyboardShortcutsSection
          keybindings={settings.keybindings}
          onChange={(patch) => update.mutate({ keybindings: patch })}
        />
        <BackupSection settings={settings} />
      </div>
    </div>
  )
}
