import type { Linked, UpdateSummary } from '@/bindings/types'

function list(names: string[]): string {
  if (names.length <= 1) return names.join('')
  return `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`
}

export function describeUpdates({ updated, waiting, failed }: UpdateSummary): string {
  const said = [
    updated.length > 0 ? `Updated ${list(updated)}.` : '',
    waiting.length > 0 ? `${list(waiting)} ${waiting.length === 1 ? 'waits' : 'wait'} for your approval below.` : '',
    failed.length > 0 ? `${list(failed)} could not be checked.` : '',
  ].filter(Boolean)
  return said.length > 0 ? said.join(' ') : 'Everything is up to date.'
}

/** What linking an extension to its store listing did. */
export function describeLinked(name: string, linked: Linked): string {
  const linkedNow = `${name} now gets its updates from the Chrome Web Store`
  if (linked === 'updated') return `${linkedNow}, and was just updated.`
  if (linked === 'waiting') return `${linkedNow}. Its update asks for more, and awaits approval in Settings.`
  return `${linkedNow}.`
}
