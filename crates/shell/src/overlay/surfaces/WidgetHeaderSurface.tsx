/**
 * Slim accent-coloured header bar docked to the top of the visible widget
 * panel, hosting a reload button (top right). Lives in its own overlay
 * webview because overlays are the only chrome UI promoted above native
 * webviews. It only reports the click; the main chrome owns which widget is
 * visible and triggers the native (cache-bypassing) reload, which works even
 * when the page inside the panel is hung.
 */

import { emit } from '@tauri-apps/api/event'

import { useBrowserState } from '@/hooks/useBrowserState'
import { useBrowserStateEvents } from '@/hooks/useBrowserStateEvents'
import { useThemeApply } from '@/hooks/useThemeApply'
import { WPANEL_RELOAD_EVENT } from '@/lib/widgetResize'

export function WidgetHeaderSurface() {
  useBrowserStateEvents()
  const { data: state } = useBrowserState()
  useThemeApply(state?.settings)

  return (
    <div className="kb-wheader-surface">
      <button
        type="button"
        className="kb-wheader-reload"
        title="Reload"
        aria-label="Reload widget"
        onClick={() => void emit(WPANEL_RELOAD_EVENT).catch(() => undefined)}
      >
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2.2"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden
        >
          <path d="M21 12a9 9 0 1 1-2.64-6.36" />
          <path d="M21 3v6h-6" />
        </svg>
      </button>
    </div>
  )
}
