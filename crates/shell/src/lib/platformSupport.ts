/**
 * Single source of truth for user-invokable features that are not available on
 * a given platform, so an invocation can raise a "{feature} not available on
 * {target}" toast instead of silently no-op'ing or surfacing a raw error.
 *
 * The lists reflect the *current* build: a feature that has a working native
 * implementation on a platform must not appear here for that platform. Keep
 * this in lockstep with the Rust cfg-gated command arms: a feature wired up on
 * Windows/Linux should be removed from its entry here.
 */

import { currentPlatform, type Platform } from '@/lib/platform'
import { showToast } from '@/lib/toast'

/** Exact platform names for the toast message. */
export const PLATFORM_LABEL: Record<Platform, string> = {
  macos: 'macOS',
  windows: 'Windows',
  linux: 'Linux',
}

interface FeatureSupport {
  /** The exact feature name shown in the toast. */
  label: string
  /** Platforms the feature is not available on. */
  unavailableOn: readonly Platform[]
}

/**
 * Keyed by a stable feature id. For features driven by a `KeyAction` the id is
 * the action id.
 */
const FEATURES: Readonly<Record<string, FeatureSupport>> = {
  // macOS reparents the webview into a floating window; Windows uses the
  // engine's native Picture-in-Picture. Linux (WebKitGTK) has neither yet.
  pop_out_video: { label: 'Video pop-out', unavailableOn: ['linux'] },
  // The provider key is sealed in the OS key store (Keychain / DPAPI), and
  // Linux has none. Mirrors `ai_supported()` in `ai.rs`.
  ai_assistant: { label: 'AI assistant', unavailableOn: ['linux'] },
  // The `wisCanvas` page channel is macOS only so far.
  canvases: { label: 'Canvases', unavailableOn: ['windows', 'linux'] },
}

/** True when `featureId` is not available on the running platform. */
export function isFeatureUnavailableHere(featureId: string): boolean {
  const feature = FEATURES[featureId]
  return feature ? feature.unavailableOn.includes(currentPlatform()) : false
}

/**
 * If `featureId` is unavailable on the running platform, show the
 * "{feature} not available on {target}" toast and return `true`. 
 * Returns `false` (and shows nothing) otherwise, including for unknown ids.
 */
export function guardFeatureUnavailable(featureId: string): boolean {
  const feature = FEATURES[featureId]
  if (!feature || !feature.unavailableOn.includes(currentPlatform())) return false
  showToast(`${feature.label} not available on ${PLATFORM_LABEL[currentPlatform()]}`)
  return true
}
