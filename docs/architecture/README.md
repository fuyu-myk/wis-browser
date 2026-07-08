# Wis architecture guides

These guides elaborate on WIS.md. If anything here conflicts, WIS.md wins.

- [`data-model-and-state.md`](data-model-and-state.md): `wis-core` immutable model, `wis-storage` schema/migrations, the IPC `mutate` pattern, coalesced/resilient persistence, and encrypted profile backup.
- [`privacy-and-network-engine.md`](privacy-and-network-engine.md): `wis-network` internals plus the privacy features layered on it.
- [`security-vault-and-permissions.md`](security-vault-and-permissions.md): `wis-secrets` crypto, the vault/biometric model, session-cookie encryption, media consent, capability isolation, CSP, navguard, and the wry patch.
- [`webviews-windows-and-overlays.md`](webviews-windows-and-overlays.md): tab/chrome compositing, overlay webviews, the one-window-per-workspace model, engine-popup routing, and window styling/glass.
- [`cross-platform-and-page-channel.md`](cross-platform-and-page-channel.md): the page to chrome channel per platform, keyboard dispatch, WebView2/GTK gotchas, native UX ports, the availability toast, and platform status.
- [`chrome-shell-and-layouts.md`](chrome-shell-and-layouts.md): React frontend structure, sidebar vs top-tabs layouts, workspaces/panes/pins/folders, theming, backgrounds, and the motion system.
- [`tabs-panels-and-media.md`](tabs-panels-and-media.md): tab memory/sessions, quick features, Peek, reader, video popout, tab audio, page capture, downloads, widgets, bookmarks, history, and browser identity.
