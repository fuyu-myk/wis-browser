//! Versioned schema migrations.
//!
//! Add new migrations by appending to `all()`. Never edit a
//! migration that has already been released: write a new one instead.

use rusqlite::Connection;
use rusqlite_migration::{MigrationDefinitionError, Migrations, M};

use crate::error::{StorageError, StorageResult};

const M0001_INITIAL: &str = r#"
CREATE TABLE workspaces (
    id              TEXT    NOT NULL PRIMARY KEY,
    position        INTEGER NOT NULL,
    name            TEXT    NOT NULL,
    accent          TEXT    NOT NULL,
    icon            TEXT,
    privacy_mode    TEXT    NOT NULL,
    active_tab_id   TEXT
);

CREATE TABLE tabs (
    id              TEXT    NOT NULL PRIMARY KEY,
    workspace_id    TEXT    NOT NULL,
    position        INTEGER NOT NULL,
    url             TEXT    NOT NULL,
    title           TEXT    NOT NULL,
    favicon_hash    TEXT,
    pinned          INTEGER NOT NULL DEFAULT 0,
    muted           INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT    NOT NULL,
    last_active_at  TEXT    NOT NULL
);
CREATE INDEX idx_tabs_workspace_position ON tabs(workspace_id, position);

CREATE TABLE essentials (
    workspace_id    TEXT    NOT NULL,
    slot            INTEGER NOT NULL,
    label           TEXT    NOT NULL,
    url             TEXT    NOT NULL,
    icon            TEXT,
    PRIMARY KEY (workspace_id, slot)
);

CREATE TABLE settings (
    key     TEXT NOT NULL PRIMARY KEY,
    value   TEXT NOT NULL
);

CREATE TABLE meta (
    key     TEXT NOT NULL PRIMARY KEY,
    value   TEXT NOT NULL
);
"#;

const M0002_PANE_TREE: &str = r#"
ALTER TABLE workspaces ADD COLUMN pane_tree TEXT;
"#;

/// Append-style browsing history, deduplicated by URL: each visit bumps
/// `visit_count` and refreshes `visited_at`/`title`. Independent of
/// `BrowserState` (it is never loaded into the in-memory model). Ephemeral
/// (private) workspaces never write here.
const M0003_HISTORY: &str = r#"
CREATE TABLE history (
    id           INTEGER NOT NULL PRIMARY KEY,
    url          TEXT    NOT NULL UNIQUE,
    title        TEXT    NOT NULL,
    visited_at   TEXT    NOT NULL,
    visit_count  INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX idx_history_visited_at ON history(visited_at);
CREATE INDEX idx_history_url ON history(url);
"#;

/// Scope history per workspace. Dedup becomes (workspace_id, url) so the same
/// page visited in two workspaces keeps an independent count/timestamp. The
/// table is recreated rather than altered because SQLite can't drop the old
/// `url UNIQUE` constraint in place; history introduced in M0003 is recent
/// enough that resetting it on upgrade is acceptable.
const M0004_HISTORY_PER_WORKSPACE: &str = r#"
DROP TABLE history;
CREATE TABLE history (
    id           INTEGER NOT NULL PRIMARY KEY,
    workspace_id TEXT    NOT NULL,
    url          TEXT    NOT NULL,
    title        TEXT    NOT NULL,
    visited_at   TEXT    NOT NULL,
    visit_count  INTEGER NOT NULL DEFAULT 1,
    UNIQUE (workspace_id, url)
);
CREATE INDEX idx_history_ws_visited_at ON history(workspace_id, visited_at);
CREATE INDEX idx_history_url ON history(url);
"#;

/// Password vault. Only the password is encrypted (AEAD ciphertext + nonce,
/// sealed under the Keychain master key, see `wis-secrets`); host and username
/// are stored in the clear so the list can render and search without unlocking.
/// Deduplicated per (host, username). Independent of `BrowserState` and never
/// touched by "clear browsing data".
const M0005_CREDENTIALS: &str = r#"
CREATE TABLE credentials (
    id           INTEGER NOT NULL PRIMARY KEY,
    host         TEXT    NOT NULL,
    username     TEXT    NOT NULL,
    ciphertext   BLOB    NOT NULL,
    nonce        BLOB    NOT NULL,
    created_at   TEXT    NOT NULL,
    updated_at   TEXT    NOT NULL,
    UNIQUE (host, username)
);
CREATE INDEX idx_credentials_host ON credentials(host);
"#;

/// Address-bar search history: the non-URL queries the user typed (deduplicated
/// by query text, each repeat bumps `count` and refreshes `searched_at`). Powers
/// the "Search history" section of the URL-bar suggestions. Independent of
/// `BrowserState`; private workspaces never write here.
const M0006_SEARCHES: &str = r#"
CREATE TABLE searches (
    id           INTEGER NOT NULL PRIMARY KEY,
    query        TEXT    NOT NULL UNIQUE,
    searched_at  TEXT    NOT NULL,
    count        INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX idx_searches_searched_at ON searches(searched_at);
"#;

/// Per-workspace upstream proxy (Tor / SOCKS5 / HTTP), stored as JSON. Null =
/// direct connection. Added as a nullable column so existing state loads as-is.
const M0007_UPSTREAM_PROXY: &str = r#"
ALTER TABLE workspaces ADD COLUMN upstream_proxy TEXT;
"#;

/// Per-tab page-zoom factor (1.0 = 100%). Nullable → existing tabs load at 1.0.
const M0008_TAB_ZOOM: &str = r#"
ALTER TABLE tabs ADD COLUMN zoom REAL;
"#;

/// Per-workspace bookmarks, organised into optional folders. Append-style and
/// independent of `BrowserState`. `folder_id` NULL = unfiled. Ephemeral
/// (private) workspaces never write here. Folders carry no foreign key; deleting
/// a folder re-parents its bookmarks to unfiled in code.
const M0009_BOOKMARKS: &str = r#"
CREATE TABLE bookmark_folders (
    id           INTEGER NOT NULL PRIMARY KEY,
    workspace_id TEXT    NOT NULL,
    name         TEXT    NOT NULL,
    position     INTEGER NOT NULL DEFAULT 0,
    created_at   TEXT    NOT NULL
);
CREATE INDEX idx_bookmark_folders_ws ON bookmark_folders(workspace_id, position);

CREATE TABLE bookmarks (
    id           INTEGER NOT NULL PRIMARY KEY,
    workspace_id TEXT    NOT NULL,
    folder_id    INTEGER,
    url          TEXT    NOT NULL,
    title        TEXT    NOT NULL,
    favicon_hash TEXT,
    position     INTEGER NOT NULL DEFAULT 0,
    created_at   TEXT    NOT NULL
);
CREATE INDEX idx_bookmarks_ws ON bookmarks(workspace_id, folder_id, position);
"#;

/// Pin flag (independent of folder): a bookmark can be pinned to the quick-access
/// row and live in a folder. `pin_position` orders the pinned row separately
/// from `position` (which orders within a folder / the unfiled list).
const M0010_BOOKMARK_PINNED: &str = r#"
ALTER TABLE bookmarks ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
ALTER TABLE bookmarks ADD COLUMN pin_position INTEGER NOT NULL DEFAULT 0;
"#;

/// Persisted session cookies per workspace. Cookies are snapshotted as JSON and
/// restored into the workspace's data store on launch. One row per workspace.
/// Ephemeral (private) workspaces never write here.
const M0011_SESSION_COOKIES: &str = r#"
CREATE TABLE session_cookies (
    workspace_id TEXT    NOT NULL PRIMARY KEY,
    data         TEXT    NOT NULL,
    updated_at   TEXT    NOT NULL
);
"#;

/// `created_at` records when a session was first persisted (it is *not* refreshed
/// on later snapshots), so a persisted login's absolute lifetime can be capped:
/// a stolen snapshot is only useful for a bounded window, and the user re-auths
/// periodically (rotating the token). Pre-existing rows default to the epoch so
/// they expire immediately (also drops any pre-encryption plaintext rows).
const M0012_SESSION_COOKIE_AGE: &str = r#"
ALTER TABLE session_cookies ADD COLUMN created_at TEXT NOT NULL
    DEFAULT '1970-01-01T00:00:00+00:00';
"#;

/// Live-tab folders (distinct from bookmarks). `tab_folders` is a JSON array on
/// the workspace (like `pane_tree`); `tabs.folder_id` records membership. Both
/// nullable so existing state loads as no folders / loose tabs.
const M0013_TAB_FOLDERS: &str = r#"
ALTER TABLE workspaces ADD COLUMN tab_folders TEXT;
ALTER TABLE tabs ADD COLUMN folder_id TEXT;
"#;

/// Completed-download history (append-only, like `history`, global rather than
/// per workspace). One row per finished download; in-flight/failed/cancelled
/// downloads live only in memory. Private/ephemeral contexts never write here.
const M0014_DOWNLOADS: &str = r#"
CREATE TABLE downloads (
    id            TEXT    NOT NULL PRIMARY KEY,
    url           TEXT    NOT NULL,
    filename      TEXT    NOT NULL,
    path          TEXT    NOT NULL,
    bytes         INTEGER NOT NULL DEFAULT 0,
    completed_at  TEXT    NOT NULL
);
CREATE INDEX idx_downloads_completed_at ON downloads(completed_at);
"#;

/// Per-workspace custom background (gradient or imported wallpaper), stored as
/// JSON like `pane_tree`. Nullable so existing state loads as theme-default.
const M0015_WORKSPACE_BACKGROUND: &str = r#"
ALTER TABLE workspaces ADD COLUMN background TEXT;
"#;

/// Custom flat workspace dot colour (overrides the accent preset's swatch).
const M0016_WORKSPACE_CUSTOM_ACCENT: &str = r#"
ALTER TABLE workspaces ADD COLUMN custom_accent TEXT;
"#;

/// Per-mode background memory (JSON), so switching background modes restores
/// the previous configuration of each.
const M0017_BACKGROUND_DRAFTS: &str = r#"
ALTER TABLE workspaces ADD COLUMN background_drafts TEXT;
"#;

/// Per-tab serialized WKWebView `interactionState` (back/forward history, scroll
/// position, form state), so a tab restores its full session across a full
/// restart, not just sleep/wake. One row per tab; independent of `BrowserState`
/// (it is its own table, so it survives `save_state`'s `clear_all`). Ephemeral
/// (private) tabs never write here; orphaned rows (closed tabs) are purged at
/// startup.
const M0018_TAB_SESSIONS: &str = r#"
CREATE TABLE tab_sessions (
    tab_id      TEXT NOT NULL PRIMARY KEY,
    data        TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
"#;

/// Recently-closed tabs, so "reopen closed tab" and its browsable list survive
/// a restart. Ephemeral (private) tabs never write here, and clearing browsing
/// history clears this too.
const M0019_CLOSED_TABS: &str = r#"
CREATE TABLE closed_tabs (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    workspace_id TEXT    NOT NULL,
    url          TEXT    NOT NULL,
    title        TEXT    NOT NULL,
    folder_id    TEXT,
    position     INTEGER NOT NULL,
    pinned       INTEGER NOT NULL,
    closed_at    TEXT    NOT NULL
);
CREATE INDEX idx_closed_tabs_closed_at ON closed_tabs(closed_at);
"#;

/// Active time per workspace, local day and site. Private workspaces never
/// write here, and clearing browsing history clears this too.
const M0020_SITE_USAGE: &str = r#"
CREATE TABLE site_usage (
    workspace_id TEXT    NOT NULL,
    day          TEXT    NOT NULL,
    site         TEXT    NOT NULL,
    seconds      INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (day, workspace_id, site)
) WITHOUT ROWID;
"#;

/// Site usage gains the local hour, for the time-of-day chart. Earlier rows
/// land in hour 0.
const M0021_SITE_USAGE_HOURS: &str = r#"
CREATE TABLE site_usage_hourly (
    day          TEXT    NOT NULL,
    hour         INTEGER NOT NULL,
    workspace_id TEXT    NOT NULL,
    site         TEXT    NOT NULL,
    seconds      INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (day, workspace_id, site, hour)
) WITHOUT ROWID;
INSERT INTO site_usage_hourly (day, hour, workspace_id, site, seconds)
    SELECT day, 0, workspace_id, site, seconds FROM site_usage;
DROP TABLE site_usage;
ALTER TABLE site_usage_hourly RENAME TO site_usage;
"#;

/// Every migration, in order. The index of the last entry is the schema
/// version a fresh database lands on.
const ALL: &[&str] = &[
    M0001_INITIAL,
    M0002_PANE_TREE,
    M0003_HISTORY,
    M0004_HISTORY_PER_WORKSPACE,
    M0005_CREDENTIALS,
    M0006_SEARCHES,
    M0007_UPSTREAM_PROXY,
    M0008_TAB_ZOOM,
    M0009_BOOKMARKS,
    M0010_BOOKMARK_PINNED,
    M0011_SESSION_COOKIES,
    M0012_SESSION_COOKIE_AGE,
    M0013_TAB_FOLDERS,
    M0014_DOWNLOADS,
    M0015_WORKSPACE_BACKGROUND,
    M0016_WORKSPACE_CUSTOM_ACCENT,
    M0017_BACKGROUND_DRAFTS,
    M0018_TAB_SESSIONS,
    M0019_CLOSED_TABS,
    M0020_SITE_USAGE,
    M0021_SITE_USAGE_HOURS,
];

fn all() -> Migrations<'static> {
    Migrations::new(ALL.iter().copied().map(M::up).collect())
}

/// Newest schema version this build understands.
pub fn supported_version() -> usize {
    ALL.len()
}

/// Schema version currently stamped on `conn`.
pub fn schema_version(conn: &Connection) -> StorageResult<usize> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    Ok(version.max(0) as usize)
}

pub fn run(conn: &mut Connection) -> StorageResult<()> {
    match all().to_latest(conn) {
        Ok(()) => Ok(()),
        Err(rusqlite_migration::Error::MigrationDefinition(
            MigrationDefinitionError::DatabaseTooFarAhead,
        )) => Err(StorageError::ProfileTooNew {
            found: schema_version(conn)?,
            supported: supported_version(),
        }),
        Err(err) => Err(err.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{all, run, schema_version, supported_version, ALL};
    use crate::error::StorageError;
    use rusqlite::Connection;
    use rusqlite_migration::{Migrations, M};

    #[test]
    fn migrations_validate() {
        all().validate().expect("migrations must be valid");
    }

    #[test]
    fn fresh_database_lands_on_supported_version() {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        run(&mut conn).expect("migrate fresh db");

        assert_eq!(
            schema_version(&conn).expect("read version"),
            supported_version()
        );
    }

    #[test]
    fn hourly_usage_keeps_daily_rows_at_hour_zero() {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        Migrations::new(ALL[..20].iter().copied().map(M::up).collect())
            .to_latest(&mut conn)
            .expect("migrate to version 20");
        conn.execute(
            "INSERT INTO site_usage (workspace_id, day, site, seconds)
             VALUES ('ws', '2026-10-11', 'a.example', 42)",
            [],
        )
        .expect("daily row");

        run(&mut conn).expect("migrate to latest");

        let (hour, seconds): (i64, i64) = conn
            .query_row(
                "SELECT hour, seconds FROM site_usage WHERE site = 'a.example'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("migrated row");
        assert_eq!((hour, seconds), (0, 42));
    }

    #[test]
    fn database_from_newer_build_reports_profile_too_new() {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        run(&mut conn).expect("migrate fresh db");

        let ahead = supported_version() + 1;
        conn.pragma_update(None, "user_version", ahead as i64)
            .expect("stamp a newer schema version");

        match run(&mut conn) {
            Err(StorageError::ProfileTooNew { found, supported }) => {
                assert_eq!(found, ahead);
                assert_eq!(supported, supported_version());
            }
            other => panic!("expected ProfileTooNew, got {other:?}"),
        }
    }
}
