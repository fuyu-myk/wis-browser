import { useEffect, useState } from 'react'
import { useQuery } from '@tanstack/react-query'

import { commands } from '@/bindings/commands'
import type { AiProvider, AiSettings, UserSettings, UserSettingsPatch } from '@/bindings/types'
import { baseUrlProblem } from '@/features/ai/lib/ai'
import { BASE_URL_SUGGESTIONS } from '@/features/ai/lib/aiEndpoints'
import { useBrowserState } from '@/hooks/useBrowserState'
import { cx } from '@/lib/cx'
import { isFeatureUnavailableHere } from '@/lib/platformSupport'
import { Segmented, Switch } from '@/components/controls'
import { SettingsGroup } from '@/components/SettingsGroup'
import { SuggestInput } from '@/components/SuggestInput'

const PROVIDER_OPTIONS = [
  { value: 'anthropic' as const, label: 'Anthropic' },
  { value: 'open_ai_compatible' as const, label: 'OpenAI-compatible' },
] satisfies ReadonlyArray<{ value: AiProvider; label: string }>

const BASE_SUGGESTIONS = BASE_URL_SUGGESTIONS.map(({ url, label }) => ({ value: url, label }))

interface AiSectionProps {
  settings: UserSettings
  onUpdate: (patch: UserSettingsPatch) => void
}

function errorMessage(error: unknown): string {
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

/**
 * BYOK AI settings: provider, model, optional base URL, and the API key. This
 * section is where the assistant is switched on: it stays dormant until a key
 * is stored, or, for an OpenAI-compatible provider, a base URL is set (a local
 * server authenticates nothing, so it needs no key). The key is write-only
 * here (stored in the OS keychain, never shown back); the rest persists
 * through the normal settings patch.
 */
export function AiSection({ settings, onUpdate }: AiSectionProps) {
  const ai = settings.ai
  // Commit text on blur/Enter, not per keystroke (each is a persisted write).
  const [modelDraft, setModelDraft] = useState(ai.model)
  const [baseDraft, setBaseDraft] = useState(ai.base_url ?? '')
  const [keyDraft, setKeyDraft] = useState('')
  const [keyPresent, setKeyPresent] = useState<boolean | null>(null)
  const [keyBusy, setKeyBusy] = useState(false)
  const [keyError, setKeyError] = useState<string | null>(null)

  const supported = !isFeatureUnavailableHere('ai_assistant')
  const keyOptional = ai.provider === 'open_ai_compatible'
  const baseProblem = baseUrlProblem(baseDraft)

  useEffect(() => {
    if (!supported) return
    void commands.aiKeyPresent().then(setKeyPresent, () => setKeyPresent(false))
  }, [supported])

  const patchAi = (next: Partial<AiSettings>) => onUpdate({ ai: { ...ai, ...next } })

  const commitModel = (value = modelDraft) => {
    if (value.trim() !== ai.model) patchAi({ model: value.trim() })
  }
  const commitBase = (value = baseDraft) => {
    const next = value.trim() === '' ? null : value.trim()
    if (next !== ai.base_url) patchAi({ base_url: next })
  }

  const workspaceId = useBrowserState().data?.active_workspace_id ?? null
  const reachable = keyPresent === true || (keyOptional && !!ai.base_url)
  const { data: models = [] } = useQuery({
    queryKey: ['ai-model-suggestions', workspaceId, ai.provider, ai.base_url, keyPresent],
    queryFn: () => commands.aiListModels(workspaceId as NonNullable<typeof workspaceId>),
    enabled: supported && workspaceId !== null && reachable,
    staleTime: 5 * 60_000,
    retry: false,
  })

  const saveKey = async () => {
    const key = keyDraft.trim()
    if (!key) return
    setKeyBusy(true)
    setKeyError(null)
    try {
      await commands.aiSetKey(key)
      setKeyDraft('')
      setKeyPresent(true)
    } catch (error: unknown) {
      setKeyError(errorMessage(error))
    } finally {
      setKeyBusy(false)
    }
  }
  const clearKey = async () => {
    setKeyBusy(true)
    setKeyError(null)
    try {
      await commands.aiClearKey()
      setKeyPresent(false)
    } catch (error: unknown) {
      setKeyError(errorMessage(error))
    } finally {
      setKeyBusy(false)
    }
  }

  if (!supported) {
    return (
      <section id="ai" className="sec">
        <h2 className="sec-h">AI assistant</h2>
        <p className="kb-vault-msg">
          The assistant is not available on Linux yet as there is no OS key store to seal
          the provider key in.
        </p>
      </section>
    )
  }

  return (
    <section id="ai" className="sec">
      <h2 className="sec-h">AI assistant</h2>
      <SettingsGroup caption="Provider">
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
            <span>
              The model identifier, exactly as the provider names it.
            </span>
          </div>
          <SuggestInput
            value={modelDraft}
            onChange={setModelDraft}
            onCommit={commitModel}
            suggestions={models.map((model) => ({ value: model.id, label: model.name }))}
            placeholder={keyOptional ? 'llama3.2' : 'claude-...'}
            ariaLabel="AI model"
          />
        </div>
        {ai.provider === 'open_ai_compatible' && (
          <div className="row">
            <div className="l">
              <span>Base URL</span>
              <span className={cx({ 'is-error': baseProblem !== null })}>
                {baseProblem ?? 'Setting one turns the assistant on. Blank uses OpenAI.'}
              </span>
            </div>
            <SuggestInput
              value={baseDraft}
              onChange={setBaseDraft}
              onCommit={commitBase}
              suggestions={BASE_SUGGESTIONS}
              placeholder="http://localhost:11434/v1"
              ariaLabel="AI base URL"
            />
          </div>
        )}
        <div className="row">
          <div className="l">
            <span>{keyOptional ? 'API key (optional)' : 'API key'}</span>
            <span>
              {keyPresent
                ? 'A key is stored in your device keychain. Enter a new one to replace it.'
                : keyOptional
                  ? 'Kept in your device keychain. Leave blank for a local server.'
                  : 'Kept in your device keychain, never on disk or synced.'}
            </span>
          </div>
          <div className="kb-key-entry">
            <input
              type="password"
              className="kb-text-input"
              placeholder={keyPresent ? '••••••••••••' : keyOptional ? 'Not needed locally' : 'sk-...'}
              value={keyDraft}
              onChange={(e) => setKeyDraft(e.currentTarget.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') void saveKey()
              }}
              aria-label="AI API key"
              autoComplete="off"
            />
            {keyPresent && (
              <button type="button" className="kb-btn" onClick={() => void clearKey()} disabled={keyBusy}>
                Remove
              </button>
            )}
            <button
              type="button"
              className="kb-btn is-primary"
              onClick={() => void saveKey()}
              disabled={keyBusy || !keyDraft.trim()}
            >
              Save
            </button>
          </div>
        </div>
        {keyError !== null && <p className="kb-vault-msg">{keyError}</p>}
      </SettingsGroup>
      <SettingsGroup caption="Assistant">
        {ai.provider === 'anthropic' && (
          <div className="row">
            <div className="l">
              <span>Show reasoning</span>
              <span>
                Streams Claude's summarized thinking into the panel. Billed as output
                tokens; needs a current Claude model.
              </span>
            </div>
            <Switch
              checked={ai.show_reasoning}
              onChange={(show_reasoning) => patchAi({ show_reasoning })}
              label="Show reasoning"
            />
          </div>
        )}
        <div className="row">
          <div className="l">
            <span>Allow web access</span>
            <span>
              Lets the model search the web and read public pages through isolated,
              invisible page loads. What it reads goes to your model.
              {keyOptional && (ai.base_url ?? '') !== '' &&
                ' Research sends about 7,000 tokens, so a server running models itself needs a context length of 16k or more; Ollama can default to 4k and cuts longer prompts without saying so.'}
            </span>
          </div>
          <Switch
            checked={ai.web_access}
            onChange={(web_access) => patchAi({ web_access })}
            label="Allow web access"
          />
        </div>
        <div className="row">
          <div className="l">
            <span>Read tabs, history and bookmarks</span>
            <span>
              Lets the model list this workspace's open tabs, read a tab's text, and search its
              history and bookmarks. What it reads goes to your model.
            </span>
          </div>
          <Switch
            checked={ai.local_data}
            onChange={(local_data) => patchAi({ local_data })}
            label="Read tabs, history and bookmarks"
          />
        </div>
        {ai.local_data && (
          <div className="row">
            <div className="l">
              <span>Let the assistant manage tabs</span>
              <span>
                Lets the model open, switch to, close and group this workspace's tabs. You approve
                each action before it happens.
              </span>
            </div>
            <Switch
              checked={ai.tab_actions}
              onChange={(tab_actions) => patchAi({ tab_actions })}
              label="Let the assistant manage tabs"
            />
          </div>
        )}
      </SettingsGroup>
    </section>
  )
}
