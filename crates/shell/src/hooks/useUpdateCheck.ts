import { useEffect } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { check } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'

/**
 * Silent auto-update check on launch. Only the `main` window runs it (so
 * secondary chrome windows don't each prompt), and it's skipped in dev where
 * there's no installed bundle to update. If the GitHub release manifest
 * advertises a newer signed build, the user is asked before it downloads,
 * installs, and relaunches. Best-effort: an offline launch or a missing
 * `latest.json` must never block the browser, so failures only hit the log.
 */
export function useUpdateCheck(): void {
  useEffect(() => {
    if (import.meta.env.DEV) return

    let label: string
    try {
      label = getCurrentWindow().label
    } catch {
      return
    }
    if (label !== 'main') return

    let cancelled = false

    void (async () => {
      try {
        const update = await check()
        if (cancelled || !update) return

        const notes = update.body ? `\n\n${update.body}` : ''
        const accepted = window.confirm(
          `Wis ${update.version} is available (you have ${update.currentVersion}).${notes}\n\n` +
            'Install now and restart?',
        )
        if (!accepted) return

        await update.downloadAndInstall()
        await relaunch()
      } catch (err) {
        // eslint-disable-next-line no-console
        console.warn('update check failed', err)
      }
    })()

    return () => {
      cancelled = true
    }
  }, [])
}
