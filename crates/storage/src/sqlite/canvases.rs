//! SQLite queries backing canvases (see `wis_core::canvas::board`).

use rusqlite::{params, Connection, OptionalExtension, Row};
use uuid::Uuid;
use wis_core::canvas::board::{CanvasDocUpdate, CanvasRecord, CanvasSummary};
use wis_core::{CanvasId, WorkspaceId};

use crate::error::{StorageError, StorageResult};

/// A workspace's canvases, most recently changed first.
pub fn list(conn: &Connection, workspace_id: WorkspaceId) -> StorageResult<Vec<CanvasSummary>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, updated_at FROM canvases WHERE workspace_id = ?1 ORDER BY updated_at DESC, name",
    )?;
    let rows = stmt.query_map(params![key(workspace_id.as_uuid())], |row| {
        Ok(CanvasSummary {
            id: CanvasId::from(read_uuid(row, 0)?),
            name: row.get(1)?,
            updated_at: row.get(2)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn load(
    conn: &Connection,
    workspace_id: WorkspaceId,
    id: CanvasId,
) -> StorageResult<Option<CanvasRecord>> {
    Ok(conn
        .query_row(
            "SELECT id, workspace_id, name, doc, created_at, updated_at FROM canvases
             WHERE id = ?1 AND workspace_id = ?2",
            params![key(id.as_uuid()), key(workspace_id.as_uuid())],
            read_record,
        )
        .optional()?)
}

pub fn insert(conn: &Connection, canvas: &CanvasRecord) -> StorageResult<()> {
    conn.execute(
        "INSERT INTO canvases (id, workspace_id, name, doc, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            key(canvas.id.as_uuid()),
            key(canvas.workspace_id.as_uuid()),
            canvas.name,
            canvas.doc,
            canvas.created_at,
            canvas.updated_at,
        ],
    )?;
    Ok(())
}

/// True when the canvas exists in that workspace.
pub fn rename(
    conn: &Connection,
    workspace_id: WorkspaceId,
    id: CanvasId,
    name: &str,
    updated_at: i64,
) -> StorageResult<bool> {
    let changed = conn.execute(
        "UPDATE canvases SET name = ?1, updated_at = ?2 WHERE id = ?3 AND workspace_id = ?4",
        params![
            name,
            updated_at,
            key(id.as_uuid()),
            key(workspace_id.as_uuid())
        ],
    )?;
    Ok(changed > 0)
}

/// Write newer documents in one transaction; returns the canvases that still
/// existed to take them.
pub fn save_docs(conn: &mut Connection, docs: &[CanvasDocUpdate]) -> StorageResult<Vec<CanvasId>> {
    let tx = conn.transaction()?;
    let mut written = Vec::new();
    {
        let mut stmt = tx.prepare(
            "UPDATE canvases SET doc = ?1, updated_at = ?2 WHERE id = ?3 AND workspace_id = ?4",
        )?;
        for update in docs {
            let changed = stmt.execute(params![
                update.doc,
                update.updated_at,
                key(update.id.as_uuid()),
                key(update.workspace_id.as_uuid()),
            ])?;
            if changed > 0 {
                written.push(update.id);
            }
        }
    }
    tx.commit()?;
    Ok(written)
}

/// True when the canvas existed in that workspace.
pub fn delete(conn: &Connection, workspace_id: WorkspaceId, id: CanvasId) -> StorageResult<bool> {
    let changed = conn.execute(
        "DELETE FROM canvases WHERE id = ?1 AND workspace_id = ?2",
        params![key(id.as_uuid()), key(workspace_id.as_uuid())],
    )?;
    Ok(changed > 0)
}

pub fn delete_workspace(conn: &Connection, workspace_id: WorkspaceId) -> StorageResult<()> {
    conn.execute(
        "DELETE FROM canvases WHERE workspace_id = ?1",
        params![key(workspace_id.as_uuid())],
    )?;
    Ok(())
}

fn key(id: &Uuid) -> String {
    id.to_string()
}

fn read_uuid(row: &Row<'_>, index: usize) -> rusqlite::Result<Uuid> {
    let text: String = row.get(index)?;
    Uuid::parse_str(&text).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            Box::new(StorageError::DataCorruption(format!(
                "invalid uuid {text}: {err}"
            ))),
        )
    })
}

fn read_record(row: &Row<'_>) -> rusqlite::Result<CanvasRecord> {
    Ok(CanvasRecord {
        id: CanvasId::from(read_uuid(row, 0)?),
        workspace_id: WorkspaceId::from(read_uuid(row, 1)?),
        name: row.get(2)?,
        doc: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("in-memory DB");
        super::super::migrations::run(&mut conn).expect("migrations");
        conn
    }

    fn canvas(ws: WorkspaceId, name: &str, at: i64) -> CanvasRecord {
        CanvasRecord {
            id: CanvasId::new(),
            workspace_id: ws,
            name: name.into(),
            doc: "{}".into(),
            created_at: at,
            updated_at: at,
        }
    }

    #[test]
    fn canvases_list_per_workspace_newest_first_and_load_whole() {
        let conn = conn();
        let (ws, other) = (WorkspaceId::new(), WorkspaceId::new());
        let old = canvas(ws, "Old", 1);
        let new = canvas(ws, "New", 2);
        insert(&conn, &old).unwrap();
        insert(&conn, &new).unwrap();
        insert(&conn, &canvas(other, "Elsewhere", 3)).unwrap();

        assert_eq!(list(&conn, ws).unwrap(), vec![new.summary(), old.summary()]);
        assert_eq!(load(&conn, ws, old.id).unwrap(), Some(old.clone()));
        assert_eq!(
            load(&conn, other, old.id).unwrap(),
            None,
            "never across workspaces"
        );
    }

    #[test]
    fn renames_and_deletes_stay_in_their_workspace() {
        let conn = conn();
        let (ws, other) = (WorkspaceId::new(), WorkspaceId::new());
        let b = canvas(ws, "Plans", 1);
        insert(&conn, &b).unwrap();

        assert!(!rename(&conn, other, b.id, "Stolen", 5).unwrap());
        assert!(rename(&conn, ws, b.id, "Trip", 5).unwrap());
        let renamed = load(&conn, ws, b.id).unwrap().unwrap();
        assert_eq!((renamed.name.as_str(), renamed.updated_at), ("Trip", 5));

        assert!(!delete(&conn, other, b.id).unwrap());
        assert!(delete(&conn, ws, b.id).unwrap());
        assert_eq!(load(&conn, ws, b.id).unwrap(), None);
    }

    #[test]
    fn saved_documents_land_only_on_canvases_that_exist() {
        let mut conn = conn();
        let ws = WorkspaceId::new();
        let b = canvas(ws, "Plans", 1);
        insert(&conn, &b).unwrap();
        let gone = CanvasId::new();

        let written = save_docs(
            &mut conn,
            &[
                CanvasDocUpdate {
                    workspace_id: ws,
                    id: b.id,
                    doc: r#"{"cards":[]}"#.into(),
                    updated_at: 9,
                },
                CanvasDocUpdate {
                    workspace_id: ws,
                    id: gone,
                    doc: "{}".into(),
                    updated_at: 9,
                },
            ],
        )
        .unwrap();

        assert_eq!(written, vec![b.id]);
        let saved = load(&conn, ws, b.id).unwrap().unwrap();
        assert_eq!(
            (saved.doc.as_str(), saved.updated_at),
            (r#"{"cards":[]}"#, 9)
        );
    }

    #[test]
    fn deleting_a_workspace_takes_only_its_canvases() {
        let conn = conn();
        let (ws, other) = (WorkspaceId::new(), WorkspaceId::new());
        insert(&conn, &canvas(ws, "A", 1)).unwrap();
        let kept = canvas(other, "B", 1);
        insert(&conn, &kept).unwrap();

        delete_workspace(&conn, ws).unwrap();

        assert!(list(&conn, ws).unwrap().is_empty());
        assert_eq!(list(&conn, other).unwrap(), vec![kept.summary()]);
    }
}
