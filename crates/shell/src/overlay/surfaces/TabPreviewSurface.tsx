/**
 * Hover tab-preview card, hosted by the `tabpreview` overlay webview. The
 * chrome positions the overlay beside the hovered tab and sends the tab's
 * identity; this surface fetches the snapshot itself (data URL) and renders
 * it over a caption, or a placeholder block when no snapshot exists (slept
 * tabs never captured, or platforms without the capture path).
 */

import { useEffect, useRef, useState } from 'react'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { commands } from '@/bindings/commands'
import type { TabId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'
import { menusGlass } from '@/lib/glass'

const TABPREVIEW_SHOW_EVENT = 'wis-tabpreview-show'
const TABPREVIEW_HIDE_EVENT = 'wis-tabpreview-hide'

interface ShowPayload {
  tabId: TabId
  title: string
  url: string
}

function hostOf(url: string): string {
  try {
    return new URL(url).host.replace(/^www\./, '') || url
  } catch {
    return url
  }
}

export function TabPreviewSurface() {
  const { data: state } = useBrowserState()
  useBrowserStateEvents()
  useThemeApply(state?.settings)
  const glassMenus = menusGlass(state?.settings)

  const [shown, setShown] = useState<ShowPayload | null>(null)
  const [image, setImage] = useState<string | null>(null)
  const currentRef = useRef<TabId | null>(null)

  useEffect(() => {
    let cancelled = false
    const unlistens: UnlistenFn[] = []

    void listen<ShowPayload>(TABPREVIEW_SHOW_EVENT, (e) => {
      const p = e.payload
      currentRef.current = p.tabId
      setShown(p)
      setImage(null)
      void commands
        .captureTabPreview(p.tabId)
        .then((url) => {
          if (currentRef.current === p.tabId) setImage(url)
        })
        .catch(() => undefined)
    }).then((fn) => {
      if (cancelled) fn()
      else unlistens.push(fn)
    })

    void listen<void>(TABPREVIEW_HIDE_EVENT, () => {
      currentRef.current = null
      setShown(null)
      setImage(null)
    }).then((fn) => {
      if (cancelled) fn()
      else unlistens.push(fn)
    })

    return () => {
      cancelled = true
      unlistens.forEach((fn) => fn())
    }
  }, [])

  if (!shown) return null

  return (
    <div className={cx('kb-tabprev', { 'glass-menus': glassMenus })}>
      <div className="kb-tabprev-shot">
        {image ? (
          <img src={image} alt="" draggable={false} />
        ) : (
          <div className="kb-tabprev-empty">
            <span className="kb-tabprev-empty-title">{shown.title}</span>
            <span className="kb-tabprev-empty-host">{hostOf(shown.url)}</span>
          </div>
        )}
      </div>
      <div className="kb-tabprev-caption">
        <span className="kb-tabprev-title">{shown.title}</span>
        <span className="kb-tabprev-host">{hostOf(shown.url)}</span>
      </div>
    </div>
  )
}
