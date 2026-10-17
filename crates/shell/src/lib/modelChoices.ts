export interface ModelChoice {
  /** `null` stands for a caller's own row, such as following another model. */
  id: string | null
  label: string
  hint?: string
}

export interface ListedModel {
  id: string
  name: string
}

export const SHOWN_MODELS = 200

/** The listed models matching `query`, then the typed id itself once the
 *  list has `settled` without it. */
export function modelChoices(models: readonly ListedModel[] | null, query: string, settled: boolean): ModelChoice[] {
  const own = query.trim()
  const q = own.toLowerCase()
  const listed = (models ?? [])
    .filter((m) => q === '' || m.id.toLowerCase().includes(q) || m.name.toLowerCase().includes(q))
    .slice(0, SHOWN_MODELS)
    .map((m): ModelChoice => (m.name !== m.id ? { id: m.id, label: m.name, hint: m.id } : { id: m.id, label: m.name }))
  const known = (models ?? []).some((m) => m.id === own)
  return own !== '' && settled && !known ? [...listed, { id: own, label: `Use “${own}”` }] : listed
}
