# Data Model & State Persistence

This guide elaborates on WIS.md. If anything here conflicts, WIS.md wins.

## wis-core: the immutable model

The pure data model has no I/O and no Tauri dependency, so it stays trivially testable.

- `BrowserState` (`crates/core/src/state/mod.rs`) is the root aggregate: `workspaces`, a flat `tabs` list, `active_workspace_id`, `settings`. A `Workspace` references tabs by ordered `TabId` (`tab_order`, `pinned_tab_ids`, `active_tab_id`), so the graph is normalized in memory, not nested.
- Every transition is a pure `with_*` builder returning a new value (`with_new_tab`, `with_tab_closed`, ...). Nothing mutates in place, which is why a mutation can snapshot the whole state cheaply for rollback and coalesced writes.
- Typed IDs (`WorkspaceId`, `TabId`, `TabFolderId`, `WidgetId` in `crates/core/src/ids.rs`) are newtype UUIDs, so a tab id can never be passed where a workspace id belongs. `Tab` is `Clone + PartialEq` but not `Eq`: it carries an `f64` zoom.
- `BrowserState::invariant_violations` is the self-check (every `tab_order`/`active_tab_id` resolves to a real tab, no dangling folder or pane refs). The storage layer runs it on load (`sqlite::validate`) and rejects a failing graph, so a corrupt row never enters the running app.

## wis-storage: normalized schema and migrations

Persistence is normalized tables, not a JSON blob, so a single field change does not rewrite the whole profile and partial reads stay cheap.

- The `Storage` trait (`crates/storage/src/traits.rs`) is the only interface upper layers code against; `SqliteStorage` is the sole production implementation (bundled SQLite via `rusqlite`, WAL, `synchronous=NORMAL`, `foreign_keys=ON`, set in `SqliteStorage::configure`).
- `save_state` (`crates/storage/src/sqlite/repo.rs::save`) writes the document graph across `workspaces`, `tabs`, `essentials`, `settings`, `meta` in one transaction. It is replace-the-world: `clear_all` deletes those tables then re-inserts, so the persisted set always matches the in-memory graph exactly (no orphan rows). Ephemeral (private) workspaces are filtered out here, so they never touch disk.
- Append-style stores are their own tables, so they survive `save_state`'s `clear_all`: `history` (per-workspace, deduped by url), `credentials` (ciphertext + nonce), `searches`, `bookmarks` (+ `bookmark_folders`), `session_cookies` (per-workspace encrypted snapshot, absolute TTL keyed on `created_at`), `downloads` (completed only), `tab_sessions` (per-tab history/scroll).
- The `meta` table holds scalars that must outlive the document rewrite: `active_workspace_id`, `windowed_workspaces`, `window_geometry`. `save_windowed_workspaces` upserts into `meta` rather than going through `save_state`, so a window-set change never rewrites the whole graph.
- Migrations are append-only `M0001..M0019` (`crates/storage/src/sqlite/migrations.rs`, `rusqlite_migration`). Never edit a shipped migration; add the next number. JSON columns carry shapes SQL cannot: `pane_tree`, `tab_folders`, `background`, `background_drafts`. A workspace's `selected_folder_id` is not stored; `assemble_workspaces` derives it from the active tab's folder, like `tab_order` and `pinned_tab_ids`.

## The IPC mutate pattern

Every state-changing command funnels through one helper so the compute/persist/rollback/broadcast sequence is identical and cannot be gotten wrong per command.

- `mutate_with` (`crates/shell/src-tauri/src/commands/mod.rs`) is the core: lock the async mutex on `BrowserState`, clone `previous` for rollback, run the pure transition to get `(next, value)`, install `next`, persist, then emit `browser-state-changed`.
- The transition closure is `FnOnce(&BrowserState) -> WisResult<(BrowserState, R)>` and must not perform I/O. All I/O (persist) happens outside it, so a transition is a pure function that is unit-testable without a database.
- On immediate-persist failure the in-memory state reverts to `previous` and the command returns `WisError::PersistenceFailed`, so memory and disk never silently diverge. The blocking `save_state` runs on `spawn_blocking` so it never stalls the tokio worker; a panic there also maps to `PersistenceFailed`.
- After success the command emits `browser-state-changed`; the frontend re-fetches `BrowserState` via React-Query (the query cache is the single JS-side source of truth). Read-only commands hold the mutex only long enough to clone the slice they return.
- Thin wrappers pick the durability: `mutate`/`mutate_unit` are coalesced (default), `mutate_immediate`/`mutate_unit_immediate` write synchronously for destructive transitions (close tab, delete workspace/folder, settings).

## Coalesced and resilient persistence

Every mutation snapshots the full state, so a naive design would fsync on every zoom tick or drag frame. The coalescer trades that for a debounced latest-wins write while keeping strict ordering.

- `SaveCoalescer` (`crates/shell/src-tauri/src/save_coalescer.rs`) keeps only the latest queued snapshot; a burst (reorders, zoom ticks, load/title sync) collapses to one write after a 200 ms quiet period. `schedule` replaces any not-yet-written snapshot instead of enqueuing another.
- Ordering safety is the hard part. Each snapshot gets a monotonic sequence from `next_seq`, stamped under the state lock so sequence order matches mutation order. `write_now` holds a `write_lock` and refuses any snapshot whose seq is `<=` the last `committed` seq (a no-op success). So a delayed debounced write can never roll the DB back behind a later immediate write that overtook it.
- Destructive transitions bypass the debounce (`Persist::Immediate`): losing a close or delete in a 200 ms window would surprise, and they are rare enough that an immediate fsync costs nothing.
- Quitting flushes: `SaveCoalescer::flush` runs in the Tauri `RunEvent::ExitRequested/Exit` hook, so the pending debounce window is written before exit.
- Crash resilience layers on top. WAL + `synchronous=NORMAL` cannot corrupt the file; only the most recent commit is at risk on power loss, bounded by the backup. `backup_to` (`crates/storage/src/sqlite/mod.rs`) writes a standalone copy via `VACUUM INTO` (one self-contained file, no `-wal`/`-shm` sidecars, consistent even with WAL active).
- `open_storage_resilient` (`crates/shell/src-tauri/src/lib.rs`) only ever sets a profile aside when SQLite itself says the file is damaged (`wis_storage::is_intact`, a read-only `PRAGMA integrity_check`). Every other failure leaves the file untouched:
  - **Busy or locked** (`StorageError::is_transient`): retried 5 times, 200ms apart. Another process may hold the file during launch.
  - **Newer than this build** (`StorageError::ProfileTooNew`, mapped from `rusqlite_migration`'s `DatabaseTooFarAhead`): the profile is intact but unreadable here, so Wis shows a native error and quits.
  - **Intact but unopenable**: same refusal. The rows are fine; something else needs fixing.
  - **Genuinely corrupt**: renamed to `wis.db.corrupt-<epoch>` and its `-wal`/`-shm` sidecars are renamed alongside it, never deleted. Recovery then restores `wis.db.bak`, falling back to a fresh profile.
- `wis.db.bak` is refreshed only when `hydrate_state` reports `StateOrigin::Disk`, so an empty first-run profile can never overwrite the last known-good copy.
- `hydrate_state` re-seeds a tab into any workspace that loaded with none.
- Dev builds run under their own identifier (`app.myk.wisdev`, `crates/shell/src-tauri/tauri.dev.conf.json`, applied by `make dev`) so `make dev` and an installed `Wis.app` never shares a profile directory.

## Encrypted profile backup

A profile is portable across machines through a passphrase-encrypted file, local-first with no server, and the passphrase key is deliberately separate from the device-bound vault key so a bundle is not tied to one machine.

- The payload shape, boundary validation, and re-mint discipline live in `wis_core::backup` (`crates/core/src/backup.rs`): `BackupPayload` carries each category as an `Option` (`settings`, `profile`, `credentials`). `validate` runs before anything is applied because a backup file is untrusted input.
- The on-disk `.wisbackup` is a versioned envelope (`crates/shell/src-tauri/src/backup.rs`): `version`, public `salt` and `nonce`, `ciphertext`. The payload is sealed with ChaCha20-Poly1305 under an Argon2id passphrase-derived key (`wis_secrets::derive_passphrase_key` + `random_salt`), distinct from the device-bound vault key so a bundle is portable. A wrong passphrase or tampered file fails to AEAD-open; a `version` above `BACKUP_VERSION` is rejected rather than misread.
- Import re-mints ids so a merge never collides with existing data. `BackupPayload::reminted` regenerates theme ids and, via `remint_profile`, regenerates every workspace/tab/folder id and remaps all references (`tab_order`, `pinned_tab_ids`, `active_tab_id`, the pane tree, bookmark/history workspace keys). Canvases get fresh ids too, and canvas tabs' `?canvas=` addresses follow them; export writes pending canvas documents first. `merge_profile` then splices only non-clashing workspaces into the live `BrowserState`, and only those workspaces' bookmarks, history and canvases are written.
- Import is merge, not replace: a custom theme appends only when absent by name and content, and a workspace whose name already exists is skipped (a re-import is a graph no-op). Private/ephemeral workspaces are never exported.
- The saved-passwords category is the only part needing the device vault key. On export, credentials are decrypted with the device master key (behind a biometric unlock) and placed plaintext inside the passphrase-sealed payload; on import they are re-encrypted under the new device's master key and upserted by `(host, username)`. Passwords apply before the profile graph, so declining biometric auth aborts the whole import before anything is written.
