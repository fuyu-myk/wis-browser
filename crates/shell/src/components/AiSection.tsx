import { useEffect, useState } from 'react'

import { commands } from '@/bindings/commands'
import type { AiProvider, AiSettings, UserSettings, UserSettingsPatch } from '@/bindings/types'
import { Segmented } from '@/components/controls'

const PROVIDER_OPTIONS = [
  { value: 'anthropic' as const, label: 'Anthropic' },
  { value: 'open_ai_compatible' as const, label: 'OpenAI-compatible' },
] satisfies ReadonlyArray<{ value: AiProvider; label: string }>

interface AiSectionProps {
  settings: UserSettings
  onUpdate: (patch: UserSettingsPatch) => void
}

/**
 * BYOK AI settings: provider, model, optional base URL, and the API key.
 * Setting a key is the sole activation switch for the assistant. The key is
 * write-only here (stored in the OS keychain, never shown back); provider and
 * model persist through the normal settings patch.
 */
export function AiSection({ settings, onUpdate }: AiSectionProps) {
  const ai = settings.ai
  // Commit text on blur/Enter, not per keystroke (each is a persisted write).
  const [modelDraft, setModelDraft] = useState(ai.model)
  const [baseDraft, setBaseDraft] = useState(ai.base_url ?? '')
  const [keyDraft, setKeyDraft] = useState('')
  const [keyPresent, setKeyPresent] = useState<boolean | null>(null)
  const [keyBusy, setKeyBusy] = useState(false)

  useEffect(() => {
    void commands.aiKeyPresent().then(setKeyPresent)
  }, [])

  const patchAi = (next: Partial<AiSettings>) => onUpdate({ ai: { ...ai, ...next } })

  const commitModel = () => {
    if (modelDraft.trim() !== ai.model) patchAi({ model: modelDraft.trim() })
  }
  const commitBase = () => {
    const next = baseDraft.trim() === '' ? null : baseDraft.trim()
    if (next !== ai.base_url) patchAi({ base_url: next })
  }

  const saveKey = async () => {
    const key = keyDraft.trim()
    if (!key) return
    setKeyBusy(true)
    try {
      await commands.aiSetKey(key)
      setKeyDraft('')
      setKeyPresent(true)
    } finally {
      setKeyBusy(false)
    }
  }
  const clearKey = async () => {
    setKeyBusy(true)
    try {
      await commands.aiClearKey()
      setKeyPresent(false)
    } finally {
      setKeyBusy(false)
    }
  }

  return (
    <section id="ai" className="sec">
      <div className="sec-h">AI assistant</div>
      <div className="row">
        <div className="l">
          <span>Provider</span>
          <span>
            Anthropic (Claude), or any OpenAI-compatible endpoint (Ollama, LM Studio,
            OpenRouter).
          </span>
        </div>
        <Segmented
          value={ai.provider}
          onChange={(provider) => patchAi({ provider })}
          options={PROVIDER_OPTIONS}
          ariaLabel="AI provider"
        />
      </div>
      <div className="row">
        <div className="l">
          <span>Model</span>
          <span>The model identifier sent to the provider (a Claude or GPT-style id).</span>
        </div>
        <input
          type="text"
          className="kb-text-input"
          placeholder="claude-..."
          value={modelDraft}
          onChange={(e) => setModelDraft(e.currentTarget.value)}
          onBlur={commitModel}
          onKeyDown={(e) => {
            if (e.key === 'Enter') e.currentTarget.blur()
          }}
          aria-label="AI model"
        />
      </div>
      {ai.provider === 'open_ai_compatible' && (
        <div className="row">
          <div className="l">
            <span>Base URL</span>
            <span>The OpenAI-compatible API base. Blank uses the OpenAI default.</span>
          </div>
          <input
            type="text"
            className="kb-text-input"
            placeholder="http://localhost:11434/v1"
            value={baseDraft}
            onChange={(e) => setBaseDraft(e.currentTarget.value)}
            onBlur={commitBase}
            onKeyDown={(e) => {
              if (e.key === 'Enter') e.currentTarget.blur()
            }}
            aria-label="AI base URL"
          />
        </div>
      )}
      <div className="row">
        <div className="l">
          <span>API key</span>
          <span>
            {keyPresent
              ? 'A key is stored in your device keychain. Enter a new one to replace it.'
              : 'Stored in your device keychain, never on disk or synced. The assistant stays off until a key is set.'}
          </span>
        </div>
        <div className="kb-shortcut-add">
          <input
            type="password"
            className="kb-text-input"
            placeholder={keyPresent ? '••••••••••••' : 'sk-...'}
            value={keyDraft}
            onChange={(e) => setKeyDraft(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === 'Enter') void saveKey()
            }}
            aria-label="AI API key"
            autoComplete="off"
          />
          <button
            type="button"
            className="kb-btn is-primary"
            onClick={() => void saveKey()}
            disabled={keyBusy || !keyDraft.trim()}
          >
            Save
          </button>
          {keyPresent && (
            <button
              type="button"
              className="kb-btn is-danger"
              onClick={() => void clearKey()}
              disabled={keyBusy}
            >
              Remove
            </button>
          )}
        </div>
      </div>
    </section>
  )
}
