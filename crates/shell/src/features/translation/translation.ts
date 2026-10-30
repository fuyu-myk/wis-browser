import type { Tab } from '@/bindings/types'

export interface TranslateButtonView {
  label: string
  pressed: boolean
}

/**
 * The address bar's translate button for `tab`: offered on a page in a
 * language the reader doesn't read, and kept while a page is translated.
 * `null` hides it.
 */
export function translateButton(
  tab: Pick<Tab, 'translated' | 'translatable_from'> | null,
): TranslateButtonView | null {
  if (!tab) return null
  if (tab.translated) return { label: 'Show original', pressed: true }
  if (tab.translatable_from) return { label: `Translate from ${tab.translatable_from}`, pressed: false }
  return null
}
