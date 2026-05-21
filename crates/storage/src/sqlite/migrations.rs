//! Versioned schema migrations.
//!
//! Add new migrations by appending to `all()`. Never edit a
//! migration that has already been released — write a new one instead.

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

fn all() -> Migrations<'static> {
    Migrations::new(vec![M::up(M0001_INITIAL)])
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
