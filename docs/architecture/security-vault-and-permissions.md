# Security, Vault & Permissions

This guide elaborates on WIS.md. If anything here conflicts, WIS.md wins.

## Capability isolation (the core invariant)

The trust boundary is **local vs remote content**, not the window.

- `capabilities/default.json` has no `remote` block, so it grants commands only to local app webviews (chrome + `overlay-*`, loading the bundled `index.html`) on windows matching glob `["main", "win-*"]`. Tab pages (`tab-<uuid>`), Peek (`peek-page-<window>`), and engine-popup windows load remote URLs and match no command-granting capability, so a page can never call a Tauri command. The one exception, `capabilities/tab-devtools.json`, grants only the devtools-toggle to `tab-*`.
- Consequence for every feature here: page-facing gates (media, vault autofill, navguard) run **app to page** via `evaluateJavaScript` / `eval`; the page posts to narrow `WKScriptMessageHandler`s, never invoking commands. Pages cannot read the vault.

## wis-secrets crypto

`crates/secrets/src/lib.rs` is the isolated crypto crate. Nothing above it reimplements sealing.

- **AEAD**: `encrypt`/`decrypt` use ChaCha20-Poly1305 with a fresh random 96-bit nonce per seal (`OsRng`). `EncryptedBlob { ciphertext, nonce }`; `decrypt` rejects a nonce whose length is not 12 (`VaultError::Malformed`) before touching the cipher.
- **Master key**: `MasterKey = Zeroizing<[u8; 32]>`, generated from the OS CSPRNG (`generate_master_key`), zeroized on drop. Plaintext keys and secrets exist only transiently and are never logged or persisted.
- **Key store surface** (`keychain_*`, cross-platform names, all `#[cfg]`-gated): macOS uses the login Keychain (`KEYCHAIN_SERVICE = "dev.wis.browser.vault"`, account `"master-key"`). Windows seals the key with DPAPI (`dpapi_unprotect`, user-scoped) into `vault-master.key` beside `wis.db`. DPAPI encrypts but does not store, so the on-disk blob is the Keychain-ACL analogue: useless to another account or a stolen disk.
- **Passphrase path**: `derive_passphrase_key` uses Argon2id, deliberately distinct from the device-bound master key so an exported profile backup decrypts on another machine. `random_salt` (16 bytes) accompanies it.

## Vault and biometric model

`src/vault.rs` is the shell re-auth layer over the keystore-held key.

- `vault_supported()` allows macOS and Windows only. The keystore protects the key at rest regardless; the biometric prompt gates the in-memory **unlock**, matching how production browsers layer biometrics over keystore keys.
- Unlock (`vault_unlock`) runs `biometrics::authenticate` (Touch ID / Windows Hello); on success it loads the key (or `create_master_key` on first use) into `AppState.vault_key`. `vault_status` reports `has_master_key` by item existence only, no prompt.
- Windows only prompts when Hello is available (`UserConsentVerifier::CheckAvailabilityAsync`); a machine with no PIN/biometric unlocks without a prompt (key stays DPAPI-sealed) rather than locking the user out. The dialog runs through desktop interop (`IUserConsentVerifierInterop` + the foreground window).
- Reveal/save/delete require `require_unlocked`; **autofill lookup does not** (`autofill_list` runs before Touch ID so the URL-bar key button can list logins), but `autofill_fill` unlocks and decrypts.
- Capture watches several submit signals (form submit, Enter in a password field, login-control click, page teardown, MutationObserver on the filled field vanishing) and pierces shadow DOM via `composedPath`, so SPA/modal logins are caught. Hosts are `www.`-stripped on save and lookup so keys match across `www`/bare.
- CSV import/export (Settings > Passwords): the pure RFC 4180 reader/writer lives in `wis_core::password_csv` (header-sniffed Chrome/Firefox/Safari shapes; hosts reduced to the vault's stored form; rows capped; `ParsedCredential` has a redacting `Debug` so passwords can never reach logs). Both directions run through `authorize_vault(reason)`, which **always** biometric-prompts even when unlocked; export additionally sits behind an explicit plaintext-file warning dialog, and import upserts via the `(host, username)` UNIQUE constraint. The file is read via `file_io::read_capped` (the read itself is bounded; `fs::metadata` pre-checks bound neither FIFOs nor files grown after the check), the house recipe for every user-picked import file.

## Session-cookie encryption

`src/cookies.rs` persists in-memory session cookies so logins survive a restart. See also `docs/architecture/privacy-and-network-engine.md` for the request path.

- WebKit keeps expiry-less session cookies in memory only. `persist_session_cookies` snapshots that set (`getAllCookies`) after each load and on a 60s timer; `restore_session_cookies` re-injects (`setCookie:`) on a workspace's first tab each run, once per workspace via `AppState.restored_cookies`.
- **Sealed at rest** with ChaCha20-Poly1305 under a **dedicated** 256-bit key (`session_key_load_or_create`, account `"session-cookie-key"` / file `session-cookie.key`), distinct from the vault master key and **not** biometric-gated, so it is available automatically at launch (mirrors Chrome Safe Storage). If the key is unavailable nothing is written (never plaintext); an undecryptable snapshot is nothing to restore.
- **Blast-radius bounds**: `created_at` is stamped once per snapshot (not refreshed), and a restored session auto-expires after `SESSION_COOKIE_TTL_DAYS = 14` (`load_session_cookies` filters by it), forcing periodic re-auth so a stolen snapshot is useful only for a bounded window. Stale snapshots are purged at launch.
- Empty snapshots never overwrite a saved one (avoids clobbering a live session when a store is momentarily released). Ephemeral workspaces are never persisted. Gated on `persist_sessions`; off skips persist/restore and wipes saved snapshots.

## Camera & microphone consent

`src/permissions.rs` + the `overlay-permission` surface. Enforced entirely app-side so the vendored wry stays a clean verbatim drop.

- `install` (every remote webview: tabs, widget panels, Peek) injects a **document-start, all-frames** script (`GATE_JS`) that wraps `MediaDevices.prototype.getUserMedia`. The real `getUserMedia` is captured in a closure the page cannot reach, and the override lands before any page script, so WebKit only ever sees requests the user already approved.
- The gate posts `{ origin, kind }` to the narrow `wisMediaPermission` handler (webview label rides a handler ivar). `handle` answers a remembered decision immediately via `evaluateJavaScript:inFrame:` into the asking frame; an unknown one emits `wis-permission-request`, shown by `usePermissionOverlay` as a glass prompt (queued one at a time). `resolve_media_permission` unblocks the page and records the choice.
- **Two sync caches** on `AppState` are read by the handler without awaiting the state lock: `media_permissions` (persisted mirror of `UserSettings.media_permissions`, keyed `<kind>|<origin>`) and `media_permissions_session` (checked first, so "just once" overrides a remembered answer). The OS still shows its own one-time TCC prompt, gated by the `NSCameraUsageDescription` / `NSMicrophoneUsageDescription` plist strings.
- Cross-platform: Windows and Linux run the same gate via builder init script + `eval`. Linux additionally needs `install_media_bridge` (a native `permission-request` -> `allow()` bridge) because WebKitGTK denies camera/mic by default, so the post-consent request still needs rubber-stamping. Known scope: modern `getUserMedia` only; a first-time **prompted** request resolves into the main frame, so cross-origin-iframe capture is not yet covered.

## CSP on local surfaces

`tauri.conf.json` `app.security.csp`. The chrome/overlay webviews are the only capability-granting surfaces and render page-controlled strings (titles, URLs, suggestions), so a strict policy is defense-in-depth behind React escaping.

- `default-src 'self'`; `script-src 'self'` (Tauri auto-nonces its injected scripts); `object-src`/`frame-src` `'none'`; `base-uri`/`form-action` `'self'`.
- `img-src 'self' data: wis-asset:` (favicons are data URLs, workspace wallpapers ride `wis-asset:`); `font-src 'self' data:`; `connect-src 'self' ipc: http://ipc.localhost`.
- `style-src` carries `'unsafe-inline'` for React style attributes; everything else is `'self'`-only because brand fonts are bundled locally (`src/styles/fonts.css` + `fonts/*.woff2`), so the chrome does no third-party fetch and renders offline.
- Applied to assets served over the tauri protocol (production). The vite dev server is uncovered, so verify CSP behavior in a bundled build.

## Navigation safety interstitial (navguard)

`src/navguard.rs` + `overlay-navguard`. Complements Peek for real navigations, in two layers.

- **Pre-navigation**: a `wisNavGuard` handler + injected script intercepts a plain (non-Option) left-click on a risky link and `preventDefault`s it before anything loads (`navigated: false`; "Take me back" just dismisses). Same-page `#` fragment clicks are skipped.
- **Page-load fallback**: `maybe_guard` on `on_page_load` Started catches what the click path cannot (address bar, JS redirects), halting via `stopLoading` (`navigated: true`; "Take me back" goes back).
- Rust re-assesses the href with `wis_core::link_safety::assess_link`. The interstitial raises for `RiskLevel::High` and for Medium that has a reason beyond `insecure_http` (`should_interrupt`); HTTPS-First already handles plain-http, so `insecure_http`-only Medium is intentionally not interrupted. Waved-through hosts live in the session-scoped `AppState.nav_guard_bypass`; "Proceed anyway" navigates and remembers the host.

## TLS-error interstitial

`TlsErrorPage.tsx`. A TLS trust failure renders a full-page interstitial in
the tab's pane (`SplitPaneHost` routes on `load_error.kind == "tls_error"`).
Copy comes from `wis_core::TlsFault`, the per-engine certificate fault
normalized purely in `load_error.rs` from NSURLError codes (macOS),
`COREWEBVIEW2_WEB_ERROR_STATUS` (Windows), and `GTlsCertificateFlags` bits
(Linux). The failure itself is surfaced by the same path as other network
errors (`didFailProvisionalNavigation` on macOS, `NavigationCompleted` /
`load-failed` on Windows/Linux), which set the tab's transient `load_error`.

No "proceed anyway" bypass is offered. Every tab webview routes through the
loopback privacy proxy, set as a Network.framework proxy config on the
webview's data store. That layer resolves TLS trust internally, so the
server-trust authentication challenge never reaches the `WKNavigationDelegate`
for the webview to override. A cert bypass would need trust evaluation to move
into the proxy or nw layer (an architectural change deferred with the
privacy-engine design). The interstitial therefore offers only "Try again".
The macOS server-trust evaluator patch (`vendor/wry`, seventh patch) is
retained but dormant for this reason; see `vendor/wry/WIS-PATCH.md`.

## link_safety heuristics

`crates/core/src/link_safety.rs::assess_link` is a pure function (host over the imperative shell), unit-tested, shared by navguard and Peek.

- **High**: unparseable URL (`malformed_url`); code/local schemes `javascript`/`data`/`vbscript`/`file`/`blob` (`dangerous_scheme`); embedded credentials before `@` (`credentials_in_url`); deceptive anchor text naming a different site than the href (`deceptive_text`).
- **Medium**: `http` (`insecure_http`); raw IP host (`ip_address_host`); punycode `xn--` label (`idn_homograph`); more than five host labels (`many_subdomains`); unusual scheme.
- **Low**: host over 60 chars (`long_host`); abuse-associated TLD (`uncommon_tld`).
- **Deceptive-text precision**: `host_from_text` treats bare dotted anchor text as a domain only when its final label is in `TEXT_DOMAIN_TLDS`, so dotted filenames as link text ("README.md", "Cargo.toml") never flag. That list is baked into the JS pre-filter so both gates agree. A subdomain of the text host (text `paypal.com`, href `login.paypal.com`) is not deceptive.

## Vendored wry patch

`vendor/wry`, applied via `[patch.crates-io]`. See `vendor/wry/WIS-PATCH.md`.

- wry 0.55.1 verbatim except `url_from_webview` no longer unwraps a nil `WKWebView.URL`. Clicking "go back" on WebKit's fraud-warning interstitial with an empty back stack returned nil and panicked inside the navigation delegate, aborting the process.
- The fix is applied verbatim from upstream PR tauri-apps/wry#1729, so dropping the vendor once that ships is a zero-behavior-change swap.
- Kept verbatim deliberately: app features never patch the vendor, so the drop stays clean. Media consent, for instance, lives entirely in the shell.
