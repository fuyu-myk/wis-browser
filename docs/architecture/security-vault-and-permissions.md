# Security, Vault & Permissions

This guide elaborates on WIS.md. If anything here conflicts, WIS.md wins.

## Capability isolation (the core invariant)

The trust boundary is **local vs remote content**, not the window.

- `capabilities/default.json` has no `remote` block, so it grants commands only to local app webviews (chrome + `overlay-*`, loading the bundled `index.html`) on windows matching glob `["main", "win-*"]`. Tab pages (`tab-<uuid>`), Peek (`peek-page-<window>`), and engine-popup windows load remote URLs and match no command-granting capability, so a page can never call a Tauri command, without exception.
- Those webviews (and the hidden AI fetcher) are also built without wry's `window.ipc` global (`webview_prefs::keep_ipc_global_off_remote_pages`, tenth patch in `vendor/wry/WIS-PATCH.md`). Wry defines it read-only on every page, which breaks a site that assigns a global of its own by that name. Such sites throw on it and draw nothing. They also open their scripts with `tauri_runtime_wry::SKIP_TAURI_SCRIPTS` (second patch in `vendor/tauri-runtime-wry/WIS-PATCH.md`), so none of Tauri's own scripts (`isTauri`, `__TAURI_INTERNALS__` with the webview's label, its IPC and `window.print`) reach a page. `withGlobalTauri` is off, so no webview gets `window.__TAURI__`. A page's `window.print()` goes to WebKit's UI delegate instead (eleventh wry patch).
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
- Reveal/save/delete require `require_unlocked`; **autofill lookup does not** (`autofill_candidates` runs before Touch ID so the key button and the saved-logins card can list logins), but `autofill_fill` unlocks and decrypts.
- Capture watches several submit signals (form submit, Enter in a password field, login-control click, page teardown, MutationObserver on the filled field vanishing) and pierces shadow DOM via `composedPath`, so SPA/modal logins are caught. Hosts are `www.`-stripped on save and lookup so keys match across `www`/bare.
- **Password generator** (`wis_secrets::password`, `generate_password` command): a draw from the OS CSPRNG over lowercase plus the selected classes (uppercase, digits, and symbols narrowed to `!#$%&*+-=?@^_~` so a password pastes safely into shells and URLs). One character of each selected class is placed first and the result shuffled, so no class can be missing and the leading characters do not betray the class order; `gen_range` avoids the modulo bias a `%` would introduce. The length clamps to 8..=128, which also keeps the one-per-class guarantee satisfiable. It sits outside the unlock gate, since it reads and writes nothing. The value is `Zeroizing` inside the crate, though it necessarily becomes a plain string crossing the IPC boundary.
- **Breach check** (`wis_secrets::breach`, `vault_check_breaches` command, Settings > Passwords): looks saved passwords up in the Pwned Passwords data set by k-anonymity. Each password is hashed with SHA-1 locally, only the first five hex characters of that hash are sent, and the service answers with every hash sharing that prefix. The remaining 35 characters are compared here, so neither the password nor its full hash leaves the machine. Requests go through the workspace's loopback proxy, so they inherit its encrypted DNS and upstream and a Tor workspace asks over Tor, and carry `Add-Padding` so response size cannot distinguish prefixes. One request per *distinct* prefix, capped at 200 passwords per run. It prompts for biometric confirmation **even when the vault is already unlocked**. It is also deliberately explicit: nothing triggers it on a timer or at launch. A whole-run budget bounds it, because a service that accepts a connection and then stalls would otherwise hold the check open for the per-request timeout multiplied by every distinct prefix, and a run in flight cannot be cancelled. The plaintexts exist only inside the block that holds the key, and nothing derived from them but the prefix is sent.
- CSV import/export (Settings > Passwords): the pure RFC 4180 reader/writer lives in `wis_core::password_csv` (header-sniffed Chrome/Firefox/Safari shapes; hosts reduced to the vault's stored form; rows capped; `ParsedCredential` has a redacting `Debug` so passwords can never reach logs). Both directions run through `authorize_vault(reason)`, which **always** biometric-prompts even when unlocked; export additionally sits behind an explicit plaintext-file warning dialog, and import upserts via the `(host, username)` UNIQUE constraint. The file is read via `file_io::read_capped` (the read itself is bounded; `fs::metadata` pre-checks bound neither FIFOs nor files grown after the check), the house recipe for every user-picked import file.

## Strong-password offer on signup

`signup.rs` injects a document-start script that reports focus on any password field not marked `current-password`, over its own `wisSignup` channel, which on macOS sits in Wis's own content world. Tab and host are read natively. The rules string and field box come from the page, so the rules are capped at 1 KB and the box is refused if non-finite, negative in size, or past a million pixels.

- **Detection is structural, never textual**. A field offers on `autocomplete="new-password"`; on declaring `passwordrules`; on a confirm box in its form (or the document when there is no form); or on being bare in a different form from one marked `current-password`, the split login-and-register layout. `current-password` never offers. A lone unmarked field with no sign-in form and no rules is the one shape left, and the key button covers it: every reported field makes it appear, with "Use a strong password".
- **The password fits the field**. `wis_secrets::recipe_for_rules` turns `passwordrules` and the field's `maxlength` into a recipe: length bounds, allowed classes, and a custom symbol set, whose `;` and `,` the parser does not split on. `max-consecutive` is left out.
- **The card sits at its field** (`lib/signupPlacement.ts`), mapped into the window with the measured scale the page menu uses, below the field or above when only that fits, and held inside the pane. The card is a separate webview and cannot keep pace with a scroll, so it steps aside instead. Scrolling over the card hides it too, so the rest of the gesture reaches the page; if the page does not scroll, the card returns after 400 ms. The card closes when the field settles outside the pane, the page goes, the tab stops being active, or focus lands on another element in the page. That last one keys off `focusin` elsewhere, not `blur`. An explicit answer settles the page until it navigates.
- **Saving is a prompt after the signup**, through the ordinary submit capture, not a save on acceptance, which would raise Touch ID mid-form while the username is still empty. Each filled password is hashed by host, so the capture knows one only Wis has seen: its prompt says so, since dismissing it locks the user out, and it is offered even over a saved login for that username, because a fresh password there is a change, not a repeat.
- **Fields inside iframes are not covered**. Detection runs in the main frame only, and anchoring inside a cross-origin frame needs coordinates the child cannot know.

## Saved logins at the field

The same script reports a sign-in field taking focus (`wis-login-field`): a password field that does not qualify as a signup, or the username before one. On a username-first page, with no password field yet, only a field declared `autocomplete="username"` counts. When the vault has logins for the host, a card at the field lists their usernames.

- **Touch ID comes after the choice**. Listing reads `autofill_candidates`, which needs no unlock; picking a login unlocks a locked vault, then `autofill_fill` decrypts and fills.
- **The fill starts from the focused field** when it is a sign-in one, so a username-first page gets its username and a split login-and-register page fills the form in use. Otherwise it takes the page's first password field, as the key button always has.
- **The page can summon the card** by focusing a sign-in field of its own, as with any browser's autofill. On macOS that is all it can do: the script and channel run in Wis's own content world, so the page cannot post a report, call the script, or patch the DOM methods it measures with. Linux does the same with a WebKitGTK script world. On Windows the script shares the page's world but posts through a private sender carrying a per-tab token, so the page cannot post a report either, though it can replace the DOM methods the script measures with. Either way, filling takes a click on the card, Touch ID when the vault is locked, and a page host matching the saved one. Clicks in the first 500 ms are ignored, as Chrome does for its autofill popup, so a click already on its way cannot pick a login, and only a tab on screen gets a card.
- **A pick settles the page** until it navigates, and focus goes back to the page so Enter submits. Without the settling, Chromium would reopen the card at once, since it re-fires `focusin` when a page regains focus. A failed fill leaves the card up.
- **Both cards share their anchoring** (`hooks/useAnchoredCard.ts`): placement, stepping aside on scroll, and closing. They also hide while the webviews do, for a tab drag, a divider drag or a split animation, and come back at the field: shifted with a pane that only moved, or placed by the page's own report when the pane changed size. Focus moving on gets 120 ms of grace, so going from username to password moves the card rather than closing and reopening it. A click elsewhere in the page, typing, or Escape in the field dismisses either card. A click on the field that already has focus brings its card back, since no `focusin` fires for it.

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

## Open in app (custom-scheme handoff)

`src/app_launch.rs` + `overlay-applaunch`. The engine cannot render non-web schemes, so every navigation-policy seam cancels them and routes to `app_launch::intake` instead of dead-ending (tabs previously rendered a load-failure page; popups swallowed the OAuth linkback silently).

- **Classification** is pure (`wis_core::app_launch`): engine schemes (http/https/about/blob/data/javascript) stay in the engine; a deny list is never launchable, covering local/app-internal surfaces (file, tauri, asset, ipc, wry, wis, vbscript) and OS handlers with one-click exploitation history (ms-msdt, ms-officecmd, ms-appinstaller, search-ms/search, smb/afp/nfs/cifs, shell); everything else (zoommtg, slack, mailto) is an app scheme behind the prompt, matching the mainstream-browser model of arbitrary handlers gated by consent rather than an allowlist.
- **Seams**: the tab builder's `on_navigation` (unconditional; the scriptlet swap shares the hook but stays privacy-gated) and `on_new_window` (before `wants_popup`, which would otherwise open a dead blank popup for non-web schemes); `peek.rs` and `widget_panel.rs` builders; and on macOS a `WisPopupNavDelegate` the `popup_chrome` decorate pass installs on engine popups (wry builds them with a nil navigationDelegate, so in-popup OAuth redirects were invisible). Windows/Linux popup redirects keep engine defaults (WebView2 shows its own open-in-app dialog; WebKitGTK drops the load).
- **Flow** mirrors the media-permission prompt: `intake` checks the `AppState.app_launch_permissions` mirror (keyed `"<scheme>|<initiator host>"`, the launching site rather than the target, always resolved at decision time from live tab/webview state so a navigated surface cannot reuse another site's remembered allow), launches or drops remembered decisions, otherwise stores a `PendingAppLaunch` and emits `wis-app-launch-request`. A duplicate key re-raises the in-flight request (a prompt that died undecided must not leave the key silently dead; the chrome queue dedupes by request id) and at most `MAX_PENDING` distinct launches wait at once (a page inventing schemes in a loop cannot grow the map or stack prompts). The `overlay-applaunch` card (Open / Cancel / remember switch, macOS shows the handler app's display name via Launch Services; it steps below the camera/mic card when both are up) answers through `app_launch_decide(request_id, ...)`; the URL never round-trips through JS. Non-remembered decisions are one-shot. Remembered ones dual-write the mirror + `UserSettings.app_launch_permissions` (Settings -> Privacy lists and edits them).
- **Known limitation**: the wry policy hook carries no frame info, so a subframe-triggered scheme navigation is attributed to the top-level page: the prompt names the top-level site and its remembered decision covers embedded third-party frames on that site. A frame-aware hook needs upstream wry/tauri surface (the vendored patches stay verbatim upstream drops); until then, remember-allow trusts every frame the site embeds.
- **Launch** hands the vetted URL to the OS: `open` (macOS), `ShellExecuteW` (Windows; not `cmd /C start`, which expands metacharacters in page-controlled URLs), `xdg-open` (Linux).

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
