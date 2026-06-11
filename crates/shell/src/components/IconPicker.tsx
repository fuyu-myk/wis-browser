/**
 * Icon picker for folders and workspaces: outline glyphs, filled glyphs, and
 * emoji tabs over the bundled registry, plus a "None" reset. Emits encoded
 * references (`glyph:` / `glyph-fill:` / `emoji:`, validated again in core).
 * The host renders it inside its own popover and owns dismissal.
 */

import { useState } from 'react'

import { cx } from '@/lib/cx'
import { EMOJI_CHOICES, GLYPH_NAMES, GlyphIcon } from '@/lib/iconGlyphs'

type Tab = 'outline' | 'filled' | 'emoji'

interface IconPickerProps {
  value: string | null
  onPick: (icon: string | null) => void
}

export function IconPicker({ value, onPick }: IconPickerProps) {
  const [tab, setTab] = useState<Tab>(
    value?.startsWith('emoji:') ? 'emoji' : value?.startsWith('glyph-fill:') ? 'filled' : 'outline',
  )

  return (
    <div className="kb-iconpick" role="group" aria-label="Icon picker">
      <div className="kb-iconpick-tabs" role="tablist">
        {(['outline', 'filled', 'emoji'] as const).map((t) => (
          <button
            key={t}
            type="button"
            role="tab"
            aria-selected={tab === t}
            className={cx({ 'is-on': tab === t })}
            onClick={() => setTab(t)}
          >
            {t === 'outline' ? 'Line' : t === 'filled' ? 'Fill' : 'Emoji'}
          </button>
        ))}
        <button
          type="button"
          className="kb-iconpick-none"
          onClick={() => onPick(null)}
          disabled={value === null}
        >
          None
        </button>
      </div>
      <div className="kb-iconpick-grid">
        {tab === 'emoji'
          ? EMOJI_CHOICES.map((e) => {
              const encoded = `emoji:${e}`
              return (
                <button
                  key={e}
                  type="button"
                  className={cx('kb-iconpick-cell', { 'is-on': value === encoded })}
                  onClick={() => onPick(encoded)}
                >
                  <span className="kb-icon-emoji">{e}</span>
                </button>
              )
            })
          : GLYPH_NAMES.map((name) => {
              const encoded = tab === 'filled' ? `glyph-fill:${name}` : `glyph:${name}`
              return (
                <button
                  key={name}
                  type="button"
                  title={name}
                  className={cx('kb-iconpick-cell', { 'is-on': value === encoded })}
                  onClick={() => onPick(encoded)}
                >
                  <GlyphIcon name={name} filled={tab === 'filled'} size={15} />
                </button>
              )
            })}
      </div>
    </div>
  )
}
