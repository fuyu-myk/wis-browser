//! Tests for the per-workspace, foldered bookmarks store.

use wis_core::WorkspaceId;
use wis_storage::{SqliteStorage, Storage};

fn store() -> SqliteStorage {
    SqliteStorage::open_in_memory().expect("in-memory DB")
}

#[test]
fn set_pinned_keeps_folder_and_reorder_pinned_orders_row() {
    let s = store();
    let ws = WorkspaceId::new();
    let folder = s.create_bookmark_folder(ws, "F").expect("folder");
    // a is unfiled; b lives inside a folder. Both can be pinned independently.
    let a = s
        .add_bookmark(ws, None, "https://a/", "A", None, false)
        .expect("a");
    let b = s
        .add_bookmark(ws, Some(folder.id), "https://b/", "B", None, false)
        .expect("b");

    s.set_bookmark_pinned(a.id, true).expect("pin a");
    s.set_bookmark_pinned(b.id, true).expect("pin b");

    let all = s.list_bookmarks(ws).expect("list");
    let by_id = |id: i64| all.iter().find(|x| x.id == id).unwrap().clone();
    // Pinning b must not move it out of its folder.
    assert_eq!(by_id(b.id).folder_id, Some(folder.id));
    assert!(by_id(a.id).pinned && by_id(b.id).pinned);

    // Reordering the pinned row sets pin_position by index, leaving folders alone.
    s.reorder_pinned_bookmarks(&[b.id, a.id]).expect("reorder");
    let all = s.list_bookmarks(ws).expect("list");
    let by_id = |id: i64| all.iter().find(|x| x.id == id).unwrap().clone();
    assert_eq!(by_id(b.id).pin_position, 0);
    assert_eq!(by_id(a.id).pin_position, 1);
    assert_eq!(by_id(b.id).folder_id, Some(folder.id), "still filed");

    // Unpinning clears the flag but keeps the folder.
    s.set_bookmark_pinned(b.id, false).expect("unpin b");
    let all = s.list_bookmarks(ws).expect("list");
    let b2 = all.iter().find(|x| x.id == b.id).unwrap();
    assert!(!b2.pinned);
    assert_eq!(b2.folder_id, Some(folder.id));
}

#[test]
fn add_with_pinned_flag_pins_it() {
    let s = store();
    let ws = WorkspaceId::new();
    let folder = s.create_bookmark_folder(ws, "F").expect("folder");
    let bm = s
        .add_bookmark(ws, Some(folder.id), "https://p/", "P", None, true)
        .expect("add pinned");
    let all = s.list_bookmarks(ws).expect("list");
    let stored = all.iter().find(|x| x.id == bm.id).unwrap();
    assert!(stored.pinned);
    assert_eq!(stored.folder_id, Some(folder.id));
}

#[test]
fn add_and_list_bookmark_in_a_folder() {
    let s = store();
    let ws = WorkspaceId::new();
    let folder = s.create_bookmark_folder(ws, "Reading").expect("folder");
    let bm = s
        .add_bookmark(
            ws,
            Some(folder.id),
            "https://example.com/",
            "Example",
            None,
            false,
        )
        .expect("add");

    let all = s.list_bookmarks(ws).expect("list");
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].id, bm.id);
    assert_eq!(all[0].folder_id, Some(folder.id));
    assert_eq!(all[0].url, "https://example.com/");
}

#[test]
fn folders_are_scoped_per_workspace() {
    let s = store();
    let a = WorkspaceId::new();
    let b = WorkspaceId::new();
    s.create_bookmark_folder(a, "A-only").expect("folder");

    assert_eq!(s.list_bookmark_folders(a).expect("a").len(), 1);
    assert!(s.list_bookmark_folders(b).expect("b").is_empty());
}

#[test]
fn deleting_a_folder_reparents_its_bookmarks_to_unfiled() {
    let s = store();
    let ws = WorkspaceId::new();
    let folder = s.create_bookmark_folder(ws, "Temp").expect("folder");
    s.add_bookmark(ws, Some(folder.id), "https://a.example/", "A", None, false)
        .expect("add");

    s.delete_bookmark_folder(folder.id).expect("delete folder");

    let folders = s.list_bookmark_folders(ws).expect("folders");
    assert!(folders.is_empty());
    let all = s.list_bookmarks(ws).expect("list");
    assert_eq!(all.len(), 1);
    assert_eq!(
        all[0].folder_id, None,
        "bookmark should be unfiled, not deleted"
    );
}

#[test]
fn move_bookmark_changes_its_folder() {
    let s = store();
    let ws = WorkspaceId::new();
    let f1 = s.create_bookmark_folder(ws, "One").expect("f1");
    let f2 = s.create_bookmark_folder(ws, "Two").expect("f2");
    let bm = s
        .add_bookmark(ws, Some(f1.id), "https://x.example/", "X", None, false)
        .expect("add");

    s.move_bookmark(bm.id, Some(f2.id)).expect("move");

    let all = s.list_bookmarks(ws).expect("list");
    assert_eq!(all[0].folder_id, Some(f2.id));
}

#[test]
fn delete_bookmark_removes_it() {
    let s = store();
    let ws = WorkspaceId::new();
    let bm = s
        .add_bookmark(ws, None, "https://gone.example/", "Gone", None, false)
        .expect("add");
    s.delete_bookmark(bm.id).expect("delete");
    assert!(s.list_bookmarks(ws).expect("list").is_empty());
}

#[test]
fn reorder_folders_sets_position_by_index() {
    let s = store();
    let ws = WorkspaceId::new();
    let a = s.create_bookmark_folder(ws, "A").expect("a");
    let b = s.create_bookmark_folder(ws, "B").expect("b");
    let c = s.create_bookmark_folder(ws, "C").expect("c");

    // New order: c, a, b.
    s.reorder_bookmark_folders(&[c.id, a.id, b.id])
        .expect("reorder");

    let folders = s.list_bookmark_folders(ws).expect("list");
    // list_folders orders by position, so the listed order reflects the reorder.
    assert_eq!(
        folders.iter().map(|f| f.id).collect::<Vec<_>>(),
        vec![c.id, a.id, b.id],
    );
    assert_eq!(folders[0].position, 0);
    assert_eq!(folders[1].position, 1);
    assert_eq!(folders[2].position, 2);
}

#[test]
fn reorder_bookmarks_sets_position_by_index() {
    let s = store();
    let ws = WorkspaceId::new();
    let f = s.create_bookmark_folder(ws, "F").expect("folder");
    let a = s
        .add_bookmark(ws, Some(f.id), "https://a/", "A", None, false)
        .expect("a");
    let b = s
        .add_bookmark(ws, Some(f.id), "https://b/", "B", None, false)
        .expect("b");
    let c = s
        .add_bookmark(ws, Some(f.id), "https://c/", "C", None, false)
        .expect("c");

    s.reorder_bookmarks(&[c.id, a.id, b.id]).expect("reorder");

    let all = s.list_bookmarks(ws).expect("list");
    let pos = |id: i64| all.iter().find(|x| x.id == id).unwrap().position;
    assert_eq!(pos(c.id), 0);
    assert_eq!(pos(a.id), 1);
    assert_eq!(pos(b.id), 2);
    // Folder membership is untouched.
    assert!(all.iter().all(|x| x.folder_id == Some(f.id)));
}

#[test]
fn rename_folder_updates_name() {
    let s = store();
    let ws = WorkspaceId::new();
    let folder = s.create_bookmark_folder(ws, "Old").expect("folder");
    s.rename_bookmark_folder(folder.id, "New").expect("rename");
    let folders = s.list_bookmark_folders(ws).expect("folders");
    assert_eq!(folders[0].name, "New");
}
