//! Stored canvases with unsaved documents laid over them, and private
//! workspaces' canvases, which live only in memory.

use std::collections::HashMap;

use wis_core::canvas::board::{
    canvas_name, check_doc, CanvasDocUpdate, CanvasRecord, CanvasSummary, DEFAULT_CANVAS_NAME,
    EMPTY_DOC,
};
use wis_core::{CanvasId, WisError, WisResult, WorkspaceId};
use wis_storage::Storage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanvasTab {
    pub workspace: WorkspaceId,
    /// A private workspace: its canvases never reach the disk.
    pub ephemeral: bool,
}

const MAX_PENDING: usize = 64;
const MAX_PRIVATE_CANVASES: usize = 200;
const MAX_PRIVATE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Default)]
pub struct CanvasMemory {
    pending: HashMap<(WorkspaceId, CanvasId), CanvasDocUpdate>,
    private: HashMap<CanvasId, CanvasRecord>,
}

fn persistence(err: impl std::fmt::Display) -> WisError {
    WisError::PersistenceFailed {
        reason: err.to_string(),
    }
}

fn not_found() -> WisError {
    WisError::InvalidInput {
        message: "that canvas no longer exists".into(),
    }
}

impl CanvasMemory {
    pub fn list(&self, storage: &dyn Storage, tab: CanvasTab) -> WisResult<Vec<CanvasSummary>> {
        let mut canvases: Vec<CanvasSummary> = if tab.ephemeral {
            self.private
                .values()
                .filter(|b| b.workspace_id == tab.workspace)
                .map(CanvasRecord::summary)
                .collect()
        } else {
            storage.list_canvases(tab.workspace).map_err(persistence)?
        };
        for canvas in &mut canvases {
            if let Some(newer) = self.pending.get(&(tab.workspace, canvas.id)) {
                canvas.updated_at = canvas.updated_at.max(newer.updated_at);
            }
        }

        canvases.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.name.cmp(&b.name))
        });

        Ok(canvases)
    }

    pub fn load(
        &self,
        storage: &dyn Storage,
        tab: CanvasTab,
        id: CanvasId,
    ) -> WisResult<CanvasRecord> {
        let stored = if tab.ephemeral {
            self.private
                .get(&id)
                .filter(|b| b.workspace_id == tab.workspace)
                .cloned()
        } else {
            storage
                .load_canvas(tab.workspace, id)
                .map_err(persistence)?
        };
        let mut canvas = stored.ok_or_else(not_found)?;
        if let Some(newer) = self.pending.get(&(tab.workspace, id)) {
            canvas.doc = newer.doc.clone();
            canvas.updated_at = canvas.updated_at.max(newer.updated_at);
        }

        Ok(canvas)
    }

    pub fn create(
        &mut self,
        storage: &dyn Storage,
        tab: CanvasTab,
        name: Option<&str>,
        now: i64,
    ) -> WisResult<CanvasSummary> {
        let name = match name {
            Some(raw) => canvas_name(raw)?,
            None => DEFAULT_CANVAS_NAME.to_string(),
        };
        let canvas = CanvasRecord {
            id: CanvasId::new(),
            workspace_id: tab.workspace,
            name,
            doc: EMPTY_DOC.into(),
            created_at: now,
            updated_at: now,
        };
        if tab.ephemeral {
            if self.private.len() >= MAX_PRIVATE_CANVASES {
                return Err(WisError::InvalidInput {
                    message: "this private workspace has too many canvases".into(),
                });
            }
            self.private.insert(canvas.id, canvas.clone());
        } else {
            storage.insert_canvas(&canvas).map_err(persistence)?;
        }

        Ok(canvas.summary())
    }

    pub fn rename(
        &mut self,
        storage: &dyn Storage,
        tab: CanvasTab,
        id: CanvasId,
        name: &str,
        now: i64,
    ) -> WisResult<CanvasSummary> {
        let name = canvas_name(name)?;
        if tab.ephemeral {
            let canvas = self.private_canvas(tab, id)?;
            canvas.name = name;
            canvas.updated_at = now;
            return Ok(canvas.summary());
        }
        if !storage
            .rename_canvas(tab.workspace, id, &name, now)
            .map_err(persistence)?
        {
            return Err(not_found());
        }

        Ok(CanvasSummary {
            id,
            name,
            updated_at: now,
        })
    }

    pub fn save(&mut self, tab: CanvasTab, id: CanvasId, doc: String, now: i64) -> WisResult<()> {
        check_doc(&doc)?;
        if tab.ephemeral {
            let others: usize = self
                .private
                .values()
                .filter(|b| b.id != id)
                .map(|b| b.doc.len())
                .sum();
            if others + doc.len() > MAX_PRIVATE_BYTES {
                return Err(WisError::InvalidInput {
                    message: "private canvases are full; this change was not kept".into(),
                });
            }

            let canvas = self.private_canvas(tab, id)?;
            canvas.doc = doc;
            canvas.updated_at = now;

            return Ok(());
        }

        let key = (tab.workspace, id);
        if !self.pending.contains_key(&key) && self.pending.len() >= MAX_PENDING {
            return Err(WisError::InvalidInput {
                message: "too many canvases are waiting to be written; try again shortly".into(),
            });
        }

        self.pending.insert(
            key,
            CanvasDocUpdate {
                workspace_id: tab.workspace,
                id,
                doc,
                updated_at: now,
            },
        );

        Ok(())
    }

    pub fn delete(&mut self, storage: &dyn Storage, tab: CanvasTab, id: CanvasId) -> WisResult<()> {
        self.pending.remove(&(tab.workspace, id));
        if tab.ephemeral {
            self.private_canvas(tab, id)?;
            self.private.remove(&id);
            return Ok(());
        }
        if !storage
            .delete_canvas(tab.workspace, id)
            .map_err(persistence)?
        {
            return Err(not_found());
        }

        Ok(())
    }

    /// A document that fails to write stays pending unless a newer one arrived.
    pub fn flush(&mut self, storage: &dyn Storage) -> WisResult<()> {
        if self.pending.is_empty() {
            return Ok(());
        }

        let docs: Vec<CanvasDocUpdate> = self.pending.drain().map(|(_, doc)| doc).collect();
        // A document for a canvas that has gone is dropped by the write.
        if let Err(err) = storage.save_canvas_docs(&docs) {
            for doc in docs {
                self.pending
                    .entry((doc.workspace_id, doc.id))
                    .or_insert(doc);
            }

            return Err(persistence(err));
        }

        Ok(())
    }

    pub fn release_workspace(&mut self, workspace: WorkspaceId) {
        self.private
            .retain(|_, canvas| canvas.workspace_id != workspace);
        self.pending.retain(|(owner, _), _| *owner != workspace);
    }

    fn private_canvas(&mut self, tab: CanvasTab, id: CanvasId) -> WisResult<&mut CanvasRecord> {
        self.private
            .get_mut(&id)
            .filter(|b| b.workspace_id == tab.workspace)
            .ok_or_else(not_found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wis_storage::SqliteStorage;

    fn setup() -> (SqliteStorage, CanvasMemory, CanvasTab) {
        let storage = SqliteStorage::open_in_memory().expect("in-memory storage");
        let tab = CanvasTab {
            workspace: WorkspaceId::new(),
            ephemeral: false,
        };
        (storage, CanvasMemory::default(), tab)
    }

    #[test]
    fn a_new_canvas_is_named_and_listed() {
        let (storage, mut memory, tab) = setup();
        let first = memory.create(&storage, tab, None, 1).unwrap();
        let second = memory.create(&storage, tab, Some("  Trip  "), 2).unwrap();
        assert_eq!(first.name, DEFAULT_CANVAS_NAME);
        assert_eq!(second.name, "Trip");
        assert_eq!(memory.list(&storage, tab).unwrap(), vec![second, first]);
    }

    #[test]
    fn saved_documents_are_read_back_before_and_after_the_write() {
        let (storage, mut memory, tab) = setup();
        let canvas = memory.create(&storage, tab, None, 1).unwrap();
        memory
            .save(tab, canvas.id, r#"{"cards":[1]}"#.into(), 5)
            .unwrap();

        assert_eq!(
            memory.load(&storage, tab, canvas.id).unwrap().doc,
            r#"{"cards":[1]}"#
        );
        assert_eq!(
            storage
                .load_canvas(tab.workspace, canvas.id)
                .unwrap()
                .unwrap()
                .doc,
            EMPTY_DOC
        );
        assert_eq!(
            memory.list(&storage, tab).unwrap()[0].updated_at,
            5,
            "listed as changed"
        );

        memory.flush(&storage).unwrap();
        assert_eq!(
            storage
                .load_canvas(tab.workspace, canvas.id)
                .unwrap()
                .unwrap()
                .doc,
            r#"{"cards":[1]}"#
        );
        assert_eq!(
            memory.load(&storage, tab, canvas.id).unwrap().doc,
            r#"{"cards":[1]}"#
        );
    }

    #[test]
    fn bad_documents_and_other_workspaces_are_refused() {
        let (storage, mut memory, tab) = setup();
        let canvas = memory.create(&storage, tab, None, 1).unwrap();
        assert!(memory.save(tab, canvas.id, "[]".into(), 2).is_err());

        let stranger = CanvasTab {
            workspace: WorkspaceId::new(),
            ephemeral: false,
        };
        assert!(memory.load(&storage, stranger, canvas.id).is_err());
        assert!(memory
            .rename(&storage, stranger, canvas.id, "Mine", 2)
            .is_err());
        assert!(memory.delete(&storage, stranger, canvas.id).is_err());
        memory
            .save(stranger, canvas.id, r#"{"x":1}"#.into(), 3)
            .unwrap();
        memory.flush(&storage).unwrap();
        assert_eq!(
            memory.load(&storage, tab, canvas.id).unwrap().doc,
            EMPTY_DOC,
            "never written across workspaces"
        );
    }

    #[test]
    fn renames_and_deletes_reach_storage_and_drop_pending_work() {
        let (storage, mut memory, tab) = setup();
        let canvas = memory.create(&storage, tab, None, 1).unwrap();
        assert_eq!(
            memory
                .rename(&storage, tab, canvas.id, "Ideas", 2)
                .unwrap()
                .name,
            "Ideas"
        );
        memory.save(tab, canvas.id, r#"{"a":1}"#.into(), 3).unwrap();
        memory.delete(&storage, tab, canvas.id).unwrap();
        memory.flush(&storage).unwrap();
        assert!(memory.list(&storage, tab).unwrap().is_empty());
        assert!(memory.load(&storage, tab, canvas.id).is_err());
    }

    #[test]
    fn unsaved_documents_and_private_canvases_are_capped() {
        let (storage, mut memory, tab) = setup();
        let first = CanvasId::new();
        memory.save(tab, first, "{}".into(), 1).unwrap();
        for _ in 1..MAX_PENDING {
            memory.save(tab, CanvasId::new(), "{}".into(), 2).unwrap();
        }
        assert!(
            memory.save(tab, CanvasId::new(), "{}".into(), 2).is_err(),
            "one too many"
        );
        memory
            .save(tab, first, r#"{"a":1}"#.into(), 3)
            .expect("a canvas already pending can always be saved again");

        let private = CanvasTab {
            workspace: WorkspaceId::new(),
            ephemeral: true,
        };
        for _ in 0..MAX_PRIVATE_CANVASES {
            memory.create(&storage, private, None, 1).unwrap();
        }
        assert!(memory.create(&storage, private, None, 1).is_err());
    }

    #[test]
    fn private_canvas_documents_share_one_byte_budget() {
        let (storage, mut memory, mut tab) = setup();
        tab.ephemeral = true;
        let doc = format!(r#"{{"pad":"{}"}}"#, "x".repeat(7 * 1024 * 1024));
        let canvases: Vec<CanvasSummary> = (0..5)
            .map(|_| memory.create(&storage, tab, None, 1).unwrap())
            .collect();
        for canvas in &canvases[..4] {
            memory.save(tab, canvas.id, doc.clone(), 2).unwrap();
        }
        assert!(memory.save(tab, canvases[4].id, doc.clone(), 2).is_err());
        memory
            .save(tab, canvases[0].id, doc, 3)
            .expect("replacing a document frees its old bytes");
    }

    #[test]
    fn private_canvases_stay_in_memory_until_their_workspace_goes() {
        let (storage, mut memory, mut tab) = setup();
        tab.ephemeral = true;
        let canvas = memory.create(&storage, tab, Some("Secret"), 1).unwrap();
        memory.save(tab, canvas.id, r#"{"a":1}"#.into(), 2).unwrap();
        memory.flush(&storage).unwrap();

        assert_eq!(storage.list_canvases(tab.workspace).unwrap(), vec![]);
        assert_eq!(
            memory.load(&storage, tab, canvas.id).unwrap().doc,
            r#"{"a":1}"#
        );
        assert_eq!(
            memory
                .rename(&storage, tab, canvas.id, "Hidden", 3)
                .unwrap()
                .name,
            "Hidden"
        );

        memory.release_workspace(tab.workspace);
        assert!(memory.list(&storage, tab).unwrap().is_empty());
    }
}
