import type { UserSettingsPatch, Widget } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { WidgetFavicon } from '@/components/WidgetFavicon'
import { WIDGET_CATALOG, makeWidget, type WidgetCatalogEntry } from '@/lib/widgets'

interface SidebarWidgetsSectionProps {
  widgets: Widget[]
  onUpdate: (patch: UserSettingsPatch) => void
}

export function SidebarWidgetsSection({ widgets, onUpdate }: SidebarWidgetsSectionProps) {
  const enabledKinds = new Set(widgets.map((w) => w.kind))

  const toggle = (entry: WidgetCatalogEntry) => {
    const next = enabledKinds.has(entry.kind)
      ? widgets.filter((w) => w.kind !== entry.kind)
      : [...widgets, makeWidget(entry)]
    onUpdate({ sidebar_widgets: next })
  }

  const chip = (entry: WidgetCatalogEntry) => {
    const on = enabledKinds.has(entry.kind)
    return (
      <button
        key={entry.kind}
        type="button"
        role="switch"
        aria-checked={on}
        className={cx('kb-widget-chip', { on })}
        onClick={() => toggle(entry)}
      >
        <WidgetFavicon url={entry.url} label={entry.label} />
        <span>{entry.label}</span>
      </button>
    )
  }

  const social = WIDGET_CATALOG.filter((e) => e.category === 'social')
  const ai = WIDGET_CATALOG.filter((e) => e.category === 'ai')

  return (
    <section id="widgets" className="sec">
      <div className="sec-h">Sidebar widgets</div>
      <div className="row">
        <div className="l">
          <span>Quick launch</span>
          <span>
            Choose the social apps and AI providers to show in the sidebar.
            Clicking one opens it in a split pane on the sidebar side.
          </span>
        </div>
      </div>
      <div className="kb-widget-group-label">Social</div>
      <div className="kb-widget-chips">{social.map(chip)}</div>
      <div className="kb-widget-sep" aria-hidden />
      <div className="kb-widget-group-label">AI providers</div>
      <div className="kb-widget-chips">{ai.map(chip)}</div>
    </section>
  )
}
