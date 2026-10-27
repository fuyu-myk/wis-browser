/**
 * Settings -> Appearance -> Themes: built-in presets vs user themes, with a
 * live editor (light/dark token pickers), duplicate/delete, and `.wistheme`
 * import/export. Edits write through the settings patch (replace-whole
 * list), debounced so picker drags coalesce; the active theme re-applies
 * live via `useThemeApply`.
 */

import { useRef, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type {
  AccentColor,
  BrowserState,
  CustomTheme,
  ModeTokens,
  ThemeId,
} from '@/bindings/types'
import { cx } from '@/lib/cx'
import { defaultModeTokens, TOKEN_FIELDS } from '@/lib/theme'

/** Built-in accent presets and their primary-500 hex (mirrors tokens.css). */
const BUILTIN_ACCENTS = [
  { value: 'primary', label: 'Periwinkle', hex: '#6b77f8' },
  { value: 'mint', label: 'Mint', hex: '#3aa17e' },
  { value: 'rose', label: 'Rose', hex: '#c9636f' },
  { value: 'sun', label: 'Sun', hex: '#d99a3b' },
  { value: 'lila', label: 'Lila', hex: '#8d7be3' },
  { value: 'azur', label: 'Azur', hex: '#3d7fc8' },
  { value: 'gray', label: 'Gray', hex: '#6e6c66' },
  { value: 'ink', label: 'Ink', hex: '#1a1a22' },
] as const
import { BROWSER_STATE_KEY, useBrowserState } from '@/hooks/useBrowserState'
import { ColorPicker, Segmented } from '@/components/controls'
import { ConfirmDialog } from '@/components/ConfirmDialog'

const APPLY_DEBOUNCE_MS = 150

/** Human-readable reason from a rejected invoke (serialized `WisError`). */
function importErrorMessage(error: unknown): string {
  if (typeof error === 'string') return error
  if (
    error !== null &&
    typeof error === 'object' &&
    'message' in error &&
    typeof (error as { message: unknown }).message === 'string'
  ) {
    return (error as { message: string }).message
  }
  return 'unknown error'
}

export function ThemesSection() {
  const { data: state } = useBrowserState()
  const qc = useQueryClient()
  const [editingId, setEditingId] = useState<ThemeId | null>(null)
  // Default the editor to the mode currently on screen, so the first edits
  // are visible immediately (editing the other mode is valid but invisible
  // until the app switches to it).
  const effectiveMode = (): 'light' | 'dark' =>
    document.body.getAttribute('data-mode') === 'dark' ? 'dark' : 'light'
  const [editMode, setEditMode] = useState<'light' | 'dark'>(effectiveMode)
  const [confirmDelete, setConfirmDelete] = useState<CustomTheme | null>(null)
  /** Local name draft so keystrokes render instantly and may be empty;
   * committed (with a fallback) on blur/Enter. */
  const [nameDraft, setNameDraft] = useState<string | null>(null)
  const [importError, setImportError] = useState<string | null>(null)
  const [copiedId, setCopiedId] = useState<ThemeId | null>(null)
  const timerRef = useRef<number | null>(null)

  // Optimistic cache patch: theme switches and edits restyle the chrome on
  // the same frame, with the command + refetch trailing behind.
  const patchState = (fn: (s: BrowserState) => BrowserState) => {
    qc.setQueryData<BrowserState>([...BROWSER_STATE_KEY], (old) => (old ? fn(old) : old))
  }

  const themes = state?.settings.custom_themes ?? []
  const activeId = state?.settings.active_theme ?? null

  const refresh = () => void qc.invalidateQueries({ queryKey: BROWSER_STATE_KEY })

  const saveList = useMutation({
    mutationFn: (custom_themes: CustomTheme[]) => commands.updateSettings({ custom_themes }),
    onSuccess: refresh,
  })
  const setAccent = useMutation({
    mutationFn: (accent: AccentColor) => commands.updateSettings({ accent }),
    onMutate: (accent) =>
      patchState((s) => ({ ...s, settings: { ...s.settings, accent } })),
    onSuccess: refresh,
  })
  const activate = useMutation({
    mutationFn: (id: ThemeId | null) => commands.setActiveTheme(id),
    onMutate: (id) =>
      patchState((s) => ({ ...s, settings: { ...s.settings, active_theme: id } })),
    onSuccess: refresh,
  })

  const updateTheme = (next: CustomTheme, debounce: boolean) => {
    const list = themes.map((t) => (t.id === next.id ? next : t))
    patchState((s) => ({ ...s, settings: { ...s.settings, custom_themes: list } }))
    if (timerRef.current !== null) window.clearTimeout(timerRef.current)
    if (debounce) {
      timerRef.current = window.setTimeout(() => saveList.mutate(list), APPLY_DEBOUNCE_MS)
    } else {
      saveList.mutate(list)
    }
  }

  const commitName = (theme: CustomTheme) => {
    const fallback = `Theme ${themes.findIndex((t) => t.id === theme.id) + 1}`
    const name = (nameDraft ?? theme.name).trim() || fallback
    setNameDraft(null)
    if (name !== theme.name) updateTheme({ ...theme, name }, false)
  }

  const createTheme = async () => {
    // Seed the accent from the currently effective primary so a fresh theme
    // is a fork of what the user sees.
    const current = getComputedStyle(document.body)
      .getPropertyValue('--primary-500')
      .trim()
      .toLowerCase()
    const accent = /^#[0-9a-f]{6}$/.test(current) ? current : undefined
    const light = defaultModeTokens(false)
    const dark = defaultModeTokens(true)
    if (accent) {
      light.accent = accent
      dark.accent = accent
    }
    const theme: CustomTheme = {
      id: crypto.randomUUID() as ThemeId,
      name: `Theme ${themes.length + 1}`,
      tokens: { light, dark },
    }
    await commands.updateSettings({ custom_themes: [...themes, theme] })
    await commands.setActiveTheme(theme.id)
    refresh()
    setEditingId(theme.id)
  }

  const duplicate = async (theme: CustomTheme) => {
    const copy: CustomTheme = {
      ...theme,
      id: crypto.randomUUID() as ThemeId,
      name: `${theme.name} copy`.slice(0, 64),
    }
    await commands.updateSettings({ custom_themes: [...themes, copy] })
    refresh()
  }

  const remove = async (theme: CustomTheme) => {
    if (activeId === theme.id) await commands.setActiveTheme(null)
    await commands.updateSettings({
      custom_themes: themes.filter((t) => t.id !== theme.id),
    })
    if (editingId === theme.id) setEditingId(null)
    refresh()
  }

  const importTheme = async () => {
    setImportError(null)
    let theme: CustomTheme | null
    try {
      theme = await commands.importTheme()
    } catch (error: unknown) {
      setImportError(importErrorMessage(error))
      return
    }
    if (!theme) return
    await commands.updateSettings({ custom_themes: [...themes, theme] })
    await commands.setActiveTheme(theme.id)
    refresh()
  }

  const copyCode = async (theme: CustomTheme) => {
    setImportError(null)
    try {
      const code = await commands.encodeThemeCode(theme)
      await navigator.clipboard.writeText(code)
      setCopiedId(theme.id)
      window.setTimeout(() => setCopiedId((id) => (id === theme.id ? null : id)), 1500)
    } catch (error: unknown) {
      setImportError(importErrorMessage(error))
    }
  }

  const pasteCode = async () => {
    setImportError(null)
    let theme: CustomTheme
    try {
      const code = await navigator.clipboard.readText()
      theme = await commands.decodeThemeCode(code)
    } catch (error: unknown) {
      setImportError(importErrorMessage(error))
      return
    }
    await commands.updateSettings({ custom_themes: [...themes, theme] })
    await commands.setActiveTheme(theme.id)
    refresh()
  }

  const editing = themes.find((t) => t.id === editingId) ?? null

  return (
    <>
      <div className="row">
        <div className="l">
          <span>Themes</span>
          <span>
            Build full chrome themes (colours for light and dark) and share
            them as .wistheme files.
          </span>
        </div>
        <div className="kb-themes-actions">
          <button type="button" className="kb-btn" onClick={() => void createTheme()}>
            New
          </button>
          <button type="button" className="kb-btn" onClick={() => void importTheme()}>
            Import
          </button>
          <button type="button" className="kb-btn" onClick={() => void pasteCode()}>
            Paste code
          </button>
        </div>
      </div>

      {importError && (
        <p className="kb-themes-error" role="alert">
          Failed to import theme: {importError}
        </p>
      )}

      <ul className="kb-themes-list">
        <li className={cx('kb-themes-row', 'is-builtin', { 'is-active': activeId === null })}>
          <button
            type="button"
            className="kb-themes-use"
            aria-pressed={activeId === null}
            onClick={() => activate.mutate(null)}
          >
            <span className={cx('kb-themes-radio', { on: activeId === null })} aria-hidden />
            Built-in
          </button>
          {activeId === null && (
            <div className="kb-themes-builtin-accents" role="group" aria-label="Built-in accent">
              {BUILTIN_ACCENTS.map((a) => (
                <button
                  key={a.value}
                  type="button"
                  className={cx('kb-themes-accent', {
                    on: state?.settings.accent === a.value,
                  })}
                  style={{ background: a.hex }}
                  title={`${a.label} ${a.hex}`}
                  aria-label={a.label}
                  aria-pressed={state?.settings.accent === a.value}
                  onClick={() => setAccent.mutate(a.value as AccentColor)}
                />
              ))}
              <span className="kb-themes-hex">
                {BUILTIN_ACCENTS.find((a) => a.value === state?.settings.accent)?.hex ??
                  '#6b77f8'}
              </span>
            </div>
          )}
        </li>
        {themes.map((t) => (
          <li key={t.id} className={cx('kb-themes-row', { 'is-active': activeId === t.id })}>
            <button
              type="button"
              className="kb-themes-use"
              aria-pressed={activeId === t.id}
              onClick={() => activate.mutate(t.id)}
            >
              <span className={cx('kb-themes-radio', { on: activeId === t.id })} aria-hidden />
              <span
                className="kb-themes-chip"
                style={{ background: t.tokens[effectiveMode()].accent }}
                aria-hidden
              />
              {t.name}
            </button>
            <div className="kb-themes-rowactions">
              <button
                type="button"
                className="kb-btn"
                aria-label={`${editingId === t.id ? 'Done editing' : 'Edit'} ${t.name}`}
                onClick={() => {
                  setNameDraft(null)
                  setEditMode(effectiveMode())
                  setEditingId((prev) => (prev === t.id ? null : t.id))
                }}
              >
                {editingId === t.id ? 'Done' : 'Edit'}
              </button>
              <button
                type="button"
                className="kb-btn"
                aria-label={`Duplicate ${t.name}`}
                onClick={() => void duplicate(t)}
              >
                Duplicate
              </button>
              <button
                type="button"
                className="kb-btn"
                aria-label={`Export ${t.name}`}
                onClick={() => void commands.exportTheme(t).catch(() => undefined)}
              >
                Export
              </button>
              <button
                type="button"
                className="kb-btn"
                aria-label={`${copiedId === t.id ? 'Copied code for' : 'Copy code for'} ${t.name}`}
                onClick={() => void copyCode(t)}
              >
                {copiedId === t.id ? 'Copied' : 'Copy code'}
              </button>
              <button
                type="button"
                className="kb-btn is-danger"
                aria-label={`Delete ${t.name}`}
                onClick={() => setConfirmDelete(t)}
              >
                Delete
              </button>
            </div>
          </li>
        ))}
      </ul>

      {editing && (
        <div className="kb-themes-editor">
          <div className="kb-themes-editor-head">
            <input
              className="kb-text-input"
              value={nameDraft ?? editing.name}
              maxLength={64}
              placeholder="Name this theme"
              aria-label="Theme name"
              onChange={(e) => setNameDraft(e.target.value)}
              onBlur={() => commitName(editing)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') commitName(editing)
              }}
            />
            <Segmented
              value={editMode}
              onChange={setEditMode}
              options={[
                { value: 'light' as const, label: 'Light' },
                { value: 'dark' as const, label: 'Dark' },
              ]}
              ariaLabel="Theme mode"
            />
          </div>
          <ul className="kb-themes-tokens">
            {TOKEN_FIELDS.map(({ key, label }) => (
              <li key={key} className="kb-themes-token">
                <ColorPicker
                  value={editing.tokens[editMode][key]}
                  label={label}
                  onChange={(hex) =>
                    updateTheme(
                      {
                        ...editing,
                        tokens: {
                          ...editing.tokens,
                          [editMode]: {
                            ...editing.tokens[editMode],
                            [key]: hex,
                          } satisfies ModeTokens,
                        },
                      },
                      true,
                    )
                  }
                />
                <span>{label}</span>
              </li>
            ))}
          </ul>
        </div>
      )}

      <ConfirmDialog
        open={confirmDelete !== null}
        title={`Delete theme "${confirmDelete?.name ?? ''}"?`}
        message="This can't be undone. Export it first to keep a copy."
        confirmLabel="Delete"
        danger
        onConfirm={() => {
          if (confirmDelete) void remove(confirmDelete)
          setConfirmDelete(null)
        }}
        onCancel={() => setConfirmDelete(null)}
      />
    </>
  )
}
