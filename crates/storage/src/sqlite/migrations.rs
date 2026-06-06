//! Versioned schema migrations.
//!
//! Add new migrations by appending to `all()`. Never edit a
//! migration that has already been released: write a new one instead.

use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};

use crate::error::StorageResult;

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

// Append-style browsing history, deduplicated by URL: each visit bumps
// `visit_count` and refreshes `visited_at`/`title`. Independent of
// `BrowserState` (it is never loaded into the in-memory model). Ephemeral
// (private) workspaces never write here.
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

// Scope history per workspace. Dedup becomes (workspace_id, url) so the same
// page visited in two workspaces keeps an independent count/timestamp. The
// table is recreated rather than altered because SQLite can't drop the old
// `url UNIQUE` constraint in place; history introduced in M0003 is recent
// enough that resetting it on upgrade is acceptable.
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

// Password vault. Only the password is encrypted (AEAD ciphertext + nonce,
// sealed under the Keychain master key, see `wis-secrets`); host and username
// are stored in the clear so the list can render and search without unlocking.
// Deduplicated per (host, username). Independent of `BrowserState` and never
// touched by "clear browsing data".
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

// Address-bar search history: the non-URL queries the user typed (deduplicated
// by query text, each repeat bumps `count` and refreshes `searched_at`). Powers
// the "Search history" section of the URL-bar suggestions. Independent of
// `BrowserState`; private workspaces never write here.
const M0006_SEARCHES: &str = r#"
CREATE TABLE searches (
    id           INTEGER NOT NULL PRIMARY KEY,
    query        TEXT    NOT NULL UNIQUE,
    searched_at  TEXT    NOT NULL,
    count        INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX idx_searches_searched_at ON searches(searched_at);
"#;

// Per-workspace upstream proxy (Tor / SOCKS5 / HTTP), stored as JSON. Null =
// direct connection. Added as a nullable column so existing state loads as-is.
const M0007_UPSTREAM_PROXY: &str = r#"
ALTER TABLE workspaces ADD COLUMN upstream_proxy TEXT;
"#;

// Per-tab page-zoom factor (1.0 = 100%). Nullable → existing tabs load at 1.0.
const M0008_TAB_ZOOM: &str = r#"
ALTER TABLE tabs ADD COLUMN zoom REAL;
"#;

fn all() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(M0001_INITIAL),
        M::up(M0002_PANE_TREE),
        M::up(M0003_HISTORY),
        M::up(M0004_HISTORY_PER_WORKSPACE),
        M::up(M0005_CREDENTIALS),
        M::up(M0006_SEARCHES),
        M::up(M0007_UPSTREAM_PROXY),
        M::up(M0008_TAB_ZOOM),
    ])
}

pub fn run(conn: &mut Connection) -> StorageResult<()> {
    all().to_latest(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::all;

    #[test]
    fn migrations_validate() {
        all().validate().expect("migrations must be valid");
    }
}
