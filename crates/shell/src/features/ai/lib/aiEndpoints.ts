export interface EndpointSuggestion {
  url: string
  label: string
}

/** Common OpenAI-compatible endpoints offered under Settings > AI > Base URL. */
export const BASE_URL_SUGGESTIONS: readonly EndpointSuggestion[] = [
  { url: 'http://localhost:11434/v1', label: 'Ollama' },
  { url: 'http://localhost:1234/v1', label: 'LM Studio' },
  { url: 'http://localhost:8080/v1', label: 'llama.cpp server' },
  { url: 'http://localhost:8000/v1', label: 'vLLM' },
  { url: 'https://openrouter.ai/api/v1', label: 'OpenRouter' },
  { url: 'https://api.openai.com/v1', label: 'OpenAI' },
]
