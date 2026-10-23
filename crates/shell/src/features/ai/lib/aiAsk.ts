/**
 * The contract for sending a selection to the assistant from elsewhere.
 *
 * The page menu lives in its own overlay webview and the assistant panel lives
 * in the chrome, so the row cannot call the panel directly. Names and payload
 * live here so the two sides cannot drift.
 */

/** Menu to chrome: open the assistant and prompt it with the question. */
export const AI_ASK_EVENT = 'wis-ai-ask'

export interface AiAskRequest {
  /** The window that asked; events reach every window's listener. */
  window: string
  /** Rises per prompt. */
  seq: number
  question: string
}
