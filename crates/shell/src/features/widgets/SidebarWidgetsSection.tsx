import { useState } from 'react'

import { commands } from '@/bindings/commands'
import type { UserSettingsPatch, Widget, WidgetId } from '@/bindings/types'
import { cx } from '@/lib/cx'
import { XIcon } from '@/lib/icons'
import { WidgetFavicon } from '@/features/widgets/WidgetFavicon'
import { Segmented } from '@/components/controls'
import {
  customWidgetUrlProblem,
  makeCustomWidget,
  makeWidget,
  MAX_CUSTOM_WIDGET_LABEL_CHARS,
  normalizeCustomWidgetUrl,
  WIDGET_CATALOG,
  type WidgetCatalogEntry,
  type WidgetCategory,
} from '@/features/widgets/widgets'
import { SettingsGroup } from '@/components/SettingsGroup'

const GROUP_OPTIONS = [
  { value: 'custom' as const, label: 'Your sites' },
  { value: 'social' as const, label: 'Social' },
  { value: 'ai' as const, label: 'AI' },
  { value: 'productivity' as const, label: 'Productivity' },
] satisfies ReadonlyArray<{ value: WidgetCategory; label: string }>

interface SidebarWidgetsSectionProps {
  widgets: Widget[]
  onUpdate: (patch: UserSettingsPatch) => void
}

export function SidebarWidgetsSection({ widgets, onUpdate }: SidebarWidgetsSectionProps) {
  const [labelDraft, setLabelDraft] = useState('')
  const [urlDraft, setUrlDraft] = useState('')
  const [groupDraft, setGroupDraft] = useState<WidgetCategory>('custom')
  /** Custom widget loaded into the form for editing; null means adding. */
  const [editingId, setEditingId] = useState<WidgetId | null>(null)

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
  const productivity = WIDGET_CATALOG.filter((e) => e.category === 'productivity')
  const customs = widgets.filter((w) => w.kind === 'custom')

  const label = labelDraft.trim()
  const urlProblem = urlDraft.trim() === '' ? null : customWidgetUrlProblem(urlDraft)
  const canAdd =
    label !== '' &&
    label.length <= MAX_CUSTOM_WIDGET_LABEL_CHARS &&
    urlDraft.trim() !== '' &&
    urlProblem === null

  const resetForm = () => {
    setEditingId(null)
    setLabelDraft('')
    setUrlDraft('')
    setGroupDraft('custom')
  }

  const startEdit = (w: Widget) => {
    setEditingId(w.id)
    setLabelDraft(w.label)
    setUrlDraft(w.url)
    setGroupDraft(w.group ?? 'custom')
  }

  const commitCustom = () => {
    if (!canAdd) return
    if (editingId !== null) {
      const nextUrl = normalizeCustomWidgetUrl(urlDraft)
      onUpdate({
        sidebar_widgets: widgets.map((w) =>
          w.id === editingId
            ? {
                ...w,
                label: labelDraft.trim(),
                url: nextUrl,
                group: groupDraft === 'custom' ? null : groupDraft,
              }
            : w,
        ),
      })
      const edited = widgets.find((w) => w.id === editingId)
      if (edited !== undefined && edited.url !== nextUrl) {
        void commands.navigateWidgetPanel(editingId, nextUrl).catch(() => undefined)
      }
    } else {
      onUpdate({
        sidebar_widgets: [...widgets, makeCustomWidget(labelDraft, urlDraft, groupDraft)],
      })
    }
    resetForm()
  }

  const removeCustom = (id: WidgetId) => {
    if (id === editingId) resetForm()
    onUpdate({ sidebar_widgets: widgets.filter((w) => w.id !== id) })
  }

  return (
    <section id="widgets" className="sec">
      <h2 className="sec-h">Sidebar widgets</h2>
      <SettingsGroup>
        <div className="row">
          <div className="l">
            <span>Quick launch</span>
            <span>
              Choose the apps to show in the sidebar, or add any site of your own.
              Clicking one opens it in a split pane on the sidebar side.
            </span>
          </div>
        </div>
        <div className="kb-widget-group-label">Social</div>
        <div className="kb-widget-chips">{social.map(chip)}</div>
        <div className="kb-widget-sep" aria-hidden />
        <div className="kb-widget-group-label">AI providers</div>
        <div className="kb-widget-chips">{ai.map(chip)}</div>
        <div className="kb-widget-sep" aria-hidden />
        <div className="kb-widget-group-label">Productivity</div>
        <div className="kb-widget-chips">{productivity.map(chip)}</div>
        <div className="kb-widget-sep" aria-hidden />
        <div className="kb-widget-group-label">Your sites</div>
        {customs.length > 0 && (
          <div className="kb-widget-chips">
            {customs.map((w) => (
              <span
                key={w.id}
                className={cx('kb-widget-chip is-custom on', { 'is-editing': w.id === editingId })}
              >
                <button
                  type="button"
                  className="kb-widget-chip-edit"
                  aria-label={w.id === editingId ? `Stop editing ${w.label}` : `Edit ${w.label}`}
                  title={`Edit ${w.label}`}
                  onClick={() => (w.id === editingId ? resetForm() : startEdit(w))}
                >
                  <WidgetFavicon url={w.url} label={w.label} />
                  <span>{w.label}</span>
                </button>
                <button
                  type="button"
                  className="kb-widget-chip-x"
                  aria-label={`Remove ${w.label}`}
                  title={`Remove ${w.label}`}
                  onClick={() => removeCustom(w.id)}
                >
                  <XIcon />
                </button>
              </span>
            ))}
          </div>
        )}
        <div className="kb-widget-add">
          <input
            type="text"
            className="kb-text-input"
            placeholder="Name"
            value={labelDraft}
            maxLength={MAX_CUSTOM_WIDGET_LABEL_CHARS}
            onChange={(e) => setLabelDraft(e.currentTarget.value)}
            aria-label="Custom widget name"
          />
          <input
            type="text"
            className="kb-text-input"
            placeholder="notion.so"
            value={urlDraft}
            onChange={(e) => setUrlDraft(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') commitCustom()
            }}
            aria-label="Custom widget URL"
          />
          <button
            type="button"
            className="kb-btn is-primary"
            onClick={commitCustom}
            disabled={!canAdd}
          >
            {editingId !== null ? 'Save' : 'Add'}
          </button>
          {editingId !== null && (
            <button type="button" className="kb-btn" onClick={resetForm}>
              Cancel
            </button>
          )}
        </div>
        <div className="kb-widget-add-group">
          <span className="kb-widget-add-group-label">Add to</span>
          <Segmented
            value={groupDraft}
            onChange={setGroupDraft}
            options={GROUP_OPTIONS}
            ariaLabel={editingId !== null ? 'Dock row for the edited widget' : 'Dock row for the new widget'}
          />
        </div>
        {urlProblem !== null && <p className="kb-widget-add-problem">{urlProblem}</p>}
      </SettingsGroup>
    </section>
  )
}
