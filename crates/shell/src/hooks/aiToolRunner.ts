import { commands } from '@/bindings/commands'
import type { AiTabRead } from '@/bindings/types'
import {
  actionApproval,
  actionDone,
  bookmarksToolContent,
  declinedOutcome,
  fetchToolContent,
  historyToolContent,
  readsLocalData,
  SEARCH_DECLINED,
  searchApproval,
  searchToolContent,
  tabName,
  tabsToolContent,
  tabToolContent,
  type ApprovalAnswer,
  type ApprovalRequest,
  type PlannedAction,
  type SharedPage,
  type TabAction,
} from '@/lib/ai'
import type { AiPageSource } from '@/hooks/useAiChat'

/** What one tool call left behind: its row, and what the model reads back. */
export interface ToolOutcome {
  ok: boolean
  /** What the segment's dropdown shows. */
  display: string
  /** What the model reads back. */
  content: string
  resultCount?: number
  /** Header for the settled segment, overriding the usual one. */
  note?: string
}

/** One turn's tool state, shared by its rounds. */
export interface ToolContext {
  /** URLs a fetch may load; search results add to it. */
  allowedUrls: Set<string>
  readTab: (source: AiPageSource) => Promise<SharedPage>
  /** Asks the user before a tab action, or a web search once the turn has read local data. */
  confirm: (request: ApprovalRequest) => Promise<ApprovalAnswer>
  /** Set once the turn has read tabs, history or bookmarks. */
  readLocal: boolean
}

/** The tool name a failure is reported under, as the model knows it. */
const FAILED_AS: Record<Exclude<PlannedAction['kind'], 'reject'>, string> = {
  search: 'search',
  fetch: 'fetch',
  tabs: 'list_tabs',
  read: 'read_tab',
  history: 'search_history',
  bookmarks: 'search_bookmarks',
  open: 'open_tabs',
  switch: 'switch_tab',
  close: 'close_tabs',
  group: 'group_tabs',
}

const TAB_ACTIONS: ReadonlySet<PlannedAction['kind']> = new Set(['open', 'switch', 'close', 'group'])

const isTabAction = (action: PlannedAction): action is TabAction => TAB_ACTIONS.has(action.kind)

const finished = (done: { note: string; content: string }, resultCount?: number): ToolOutcome => ({
  ok: true,
  display: done.content,
  content: done.content,
  note: done.note,
  resultCount,
})

const succeeded = (content: string, resultCount?: number): ToolOutcome => ({
  ok: true,
  display: content,
  content,
  resultCount,
})

async function executeAction(
  action: Exclude<PlannedAction, { kind: 'reject' }>,
  ctx: ToolContext,
): Promise<ToolOutcome> {
  switch (action.kind) {
    case 'search': {
      const results = await commands.aiWebSearch(action.query)
      for (const r of results) ctx.allowedUrls.add(r.url)
      return succeeded(searchToolContent(results), results.length)
    }
    case 'fetch':
      return succeeded(fetchToolContent(await commands.aiWebFetch(action.url)))
    case 'tabs': {
      const tabs = await commands.aiListTabs()
      return succeeded(tabsToolContent(tabs), tabs.length)
    }
    case 'read': {
      const target = await commands.aiResolveTab(action.tab)
      const page = await ctx.readTab(target)
      return succeeded(tabToolContent({ ...page, url: target.url }))
    }
    case 'history': {
      const hits = await commands.aiSearchHistory(action.query)
      return succeeded(historyToolContent(hits), hits.length)
    }
    case 'bookmarks': {
      const hits = await commands.aiSearchBookmarks(action.query)
      return succeeded(bookmarksToolContent(hits), hits.length)
    }
    case 'open': {
      const opened = await commands.aiOpenTabs(action.urls)
      return finished(actionDone('open', opened), opened.length)
    }
    case 'switch': {
      const tab = await commands.aiSwitchTab(action.tab)
      return finished(actionDone('switch', [tabName(tab)]))
    }
    case 'close': {
      const closed = await commands.aiCloseTabs(action.tabs)
      return finished(actionDone('close', closed.map(tabName)), closed.length)
    }
    case 'group': {
      const grouped = await commands.aiGroupTabs(action.tabs, action.folder)
      const done = actionDone('group', grouped.tabs.map(tabName), {
        name: grouped.folder,
        created: grouped.created,
      })
      return finished(done, grouped.tabs.length)
    }
  }
}

type Approved = { action: Exclude<PlannedAction, { kind: 'reject' }> } | { declined: ToolOutcome }

const CANCELLED = declinedOutcome('cancel')

/** A call the user turned down, or that a stop left behind, settled as a plain row. */
const declined = ({ note, content }: { note: string; content: string }): Approved => ({
  declined: { ok: true, display: content, content, note },
})

/** The action bound to what its approval card showed: full tab ids, parsed URLs. */
function boundTo(action: TabAction, tabs: readonly AiTabRead[], urls: readonly string[]): TabAction {
  const ids = tabs.map((t) => t.id)
  switch (action.kind) {
    case 'open':
      return { ...action, urls: [...urls] }
    case 'switch':
      return { ...action, tab: ids[0] ?? action.tab }
    case 'close':
    case 'group':
      return { ...action, tabs: ids }
  }
}

/**
 * Ask the user first where a call needs it: every tab action, and a web search
 * once the turn has read local data. An accepted tab action runs on exactly
 * what its card showed. `live` turns false once a stop, Answer now or
 * workspace change abandons the turn, so no card outlives it.
 */
async function askFirst(
  action: Exclude<PlannedAction, { kind: 'reject' }>,
  ctx: ToolContext,
  live: () => boolean,
): Promise<Approved> {
  if (action.kind === 'search') {
    if (!ctx.readLocal || (await ctx.confirm(searchApproval(action.query))) === 'accept') {
      return { action }
    }
    return declined({ note: 'search declined', content: SEARCH_DECLINED })
  }
  if (!isTabAction(action)) return { action }
  const named = action.kind === 'switch' ? [action.tab] : action.kind === 'open' ? [] : action.tabs
  const urls = action.kind === 'open' ? await commands.aiPreviewUrls(action.urls) : []
  const tabs = named.length > 0 ? await commands.aiPreviewTabs(named) : []
  if (!live()) return declined(CANCELLED)
  const answer = await ctx.confirm(actionApproval(action, { tabs, urls }, ctx.readLocal))
  if (answer !== 'accept') return declined(declinedOutcome(answer))
  return live() ? { action: boundTo(action, tabs, urls) } : declined(CANCELLED)
}

/**
 * Run one vetted tool call. Every native command re-checks its own setting,
 * so a call the model was never offered still fails safe. Once the turn has
 * read local data, a web search waits for the user, since its query is the
 * one place that data could leave for a third party.
 */
export async function runToolAction(
  action: PlannedAction,
  ctx: ToolContext,
  live: () => boolean,
): Promise<ToolOutcome> {
  if (action.kind === 'reject') return { ok: false, display: action.reason, content: action.reason }
  try {
    const approved = await askFirst(action, ctx, live)
    if ('declined' in approved) return approved.declined
    const outcome = await executeAction(approved.action, ctx)
    if (readsLocalData(action.kind)) ctx.readLocal = true
    return outcome
  } catch (error: unknown) {
    const display = errorMessage(error)
    return { ok: false, display, content: `${FAILED_AS[action.kind]} failed: ${display}` }
  }
}

export function errorMessage(error: unknown): string {
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

