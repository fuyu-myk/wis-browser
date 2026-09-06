//! Tests for the append-style, per-workspace browsing-history store.

use wis_core::WorkspaceId;
use wis_storage::{HistoryEntry, HistoryQuery, SqliteStorage, Storage};

fn store() -> SqliteStorage {
    SqliteStorage::open_in_memory().expect("in-memory DB")
}

/// All rows for one workspace, newest-first.
fn ws_rows(store: &SqliteStorage, ws: WorkspaceId) -> Vec<HistoryEntry> {
    store
        .query_history(&HistoryQuery {
            workspace_id: Some(ws),
            ..Default::default()
        })
        .expect("query")
}

/// All rows across every workspace.
fn all_rows(store: &SqliteStorage) -> Vec<HistoryEntry> {
    store
        .query_history(&HistoryQuery::default())
        .expect("query")
}

#[test]
fn recording_a_visit_makes_it_queryable_in_its_workspace() {
    let s = store();
    let ws = WorkspaceId::new();
    s.record_visit(ws, "https://example.com/", "Example")
        .unwrap();

    let rows = ws_rows(&s, ws);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].url, "https://example.com/");
    assert_eq!(rows[0].title, "Example");
    assert_eq!(rows[0].workspace_id, ws);
    assert_eq!(rows[0].visit_count, 1);
}

#[test]
fn revisiting_a_url_dedups_within_a_workspace() {
    let s = store();
    let ws = WorkspaceId::new();
    s.record_visit(ws, "https://example.com/", "Example")
        .unwrap();
    s.record_visit(ws, "https://example.com/", "Example")
        .unwrap();
    s.record_visit(ws, "https://example.com/", "Example")
        .unwrap();

    let rows = ws_rows(&s, ws);
    assert_eq!(rows.len(), 1, "one row per (workspace, url)");
    assert_eq!(rows[0].visit_count, 3);
}

#[test]
fn fragment_variants_dedupe_to_one_row() {
    let s = store();
    let ws = WorkspaceId::new();
    s.record_visit(ws, "https://example.com/doc", "Doc")
        .unwrap();
    s.record_visit(ws, "https://example.com/doc#intro", "Doc")
        .unwrap();
    s.record_visit(ws, "https://example.com/doc#usage", "Doc")
        .unwrap();

    let rows = ws_rows(&s, ws);
    assert_eq!(rows.len(), 1, "anchor jumps update the fragmentless row");
    assert_eq!(rows[0].url, "https://example.com/doc");
    assert_eq!(rows[0].visit_count, 3);
}

#[test]
fn query_and_fragment_distinguish_correctly() {
    let s = store();
    let ws = WorkspaceId::new();
    // Different queries are different pages and keep separate rows.
    s.record_visit(ws, "https://example.com/search?q=a", "A")
        .unwrap();
    s.record_visit(ws, "https://example.com/search?q=b", "B")
        .unwrap();

    let rows = ws_rows(&s, ws);
    assert_eq!(rows.len(), 2);
}

#[test]
fn the_same_url_in_two_workspaces_is_independent() {
    let s = store();
    let a = WorkspaceId::new();
    let b = WorkspaceId::new();
    s.record_visit(a, "https://shared.example/", "Shared")
        .unwrap();
    s.record_visit(a, "https://shared.example/", "Shared")
        .unwrap();
    s.record_visit(b, "https://shared.example/", "Shared")
        .unwrap();

    assert_eq!(ws_rows(&s, a)[0].visit_count, 2);
    assert_eq!(ws_rows(&s, b)[0].visit_count, 1);
    assert_eq!(
        all_rows(&s).len(),
        2,
        "one row per workspace for the same URL"
    );
}

#[test]
fn querying_one_workspace_excludes_others() {
    let s = store();
    let a = WorkspaceId::new();
    let b = WorkspaceId::new();
    s.record_visit(a, "https://a.example/", "A").unwrap();
    s.record_visit(b, "https://b.example/", "B").unwrap();

    let only_a = ws_rows(&s, a);
    assert_eq!(only_a.len(), 1);
    assert_eq!(only_a[0].url, "https://a.example/");

    assert_eq!(all_rows(&s).len(), 2, "no scope returns both workspaces");
}

#[test]
fn a_blank_title_does_not_overwrite_a_known_one() {
    let s = store();
    let ws = WorkspaceId::new();
    s.record_visit(ws, "https://example.com/", "Real Title")
        .unwrap();
    s.record_visit(ws, "https://example.com/", "").unwrap();

    let rows = ws_rows(&s, ws);
    assert_eq!(rows[0].title, "Real Title");
    assert_eq!(rows[0].visit_count, 2);
}

#[test]
fn results_are_newest_first() {
    let s = store();
    let ws = WorkspaceId::new();
    s.record_visit(ws, "https://a.example/", "A").unwrap();
    s.record_visit(ws, "https://b.example/", "B").unwrap();
    s.record_visit(ws, "https://c.example/", "C").unwrap();

    let urls: Vec<_> = ws_rows(&s, ws).into_iter().map(|e| e.url).collect();
    assert_eq!(
        urls,
        vec![
            "https://c.example/",
            "https://b.example/",
            "https://a.example/"
        ]
    );
}

#[test]
fn search_filters_on_url_and_title_case_insensitively() {
    let s = store();
    let ws = WorkspaceId::new();
    s.record_visit(ws, "https://rust-lang.org/", "Rust")
        .unwrap();
    s.record_visit(ws, "https://python.org/", "Python").unwrap();

    let by_title = s
        .query_history(&HistoryQuery {
            search: "rust".into(),
            workspace_id: Some(ws),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(by_title.len(), 1);
    assert_eq!(by_title[0].url, "https://rust-lang.org/");
}

#[test]
fn limit_and_offset_paginate() {
    let s = store();
    let ws = WorkspaceId::new();
    for i in 0..5 {
        s.record_visit(ws, &format!("https://e{i}.example/"), &format!("E{i}"))
            .unwrap();
    }

    let page1 = s
        .query_history(&HistoryQuery {
            workspace_id: Some(ws),
            limit: 2,
            offset: 0,
            ..Default::default()
        })
        .unwrap();
    let page2 = s
        .query_history(&HistoryQuery {
            workspace_id: Some(ws),
            limit: 2,
            offset: 2,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(page1[0].url, "https://e4.example/");
    assert_eq!(page2[0].url, "https://e2.example/");
}

#[test]
fn deleting_entries_removes_only_those_ids() {
    let s = store();
    let ws = WorkspaceId::new();
    s.record_visit(ws, "https://keep.example/", "Keep").unwrap();
    s.record_visit(ws, "https://drop.example/", "Drop").unwrap();

    let drop_id = ws_rows(&s, ws)
        .into_iter()
        .find(|e| e.url == "https://drop.example/")
        .unwrap()
        .id;
    s.delete_history_entries(&[drop_id]).unwrap();

    let urls: Vec<_> = ws_rows(&s, ws).into_iter().map(|e| e.url).collect();
    assert_eq!(urls, vec!["https://keep.example/"]);
}

#[test]
fn update_visit_title_changes_title_without_bumping_count() {
    let s = store();
    let ws = WorkspaceId::new();
    s.record_visit(ws, "https://v.example/", "youtube.com")
        .unwrap();
    s.update_visit_title(ws, "https://v.example/", "Real Video Title")
        .unwrap();

    let rows = ws_rows(&s, ws);
    assert_eq!(rows[0].title, "Real Video Title");
    assert_eq!(
        rows[0].visit_count, 1,
        "a title patch must not bump the count"
    );
}

#[test]
fn update_visit_title_ignores_empty_and_unknown_urls() {
    let s = store();
    let ws = WorkspaceId::new();
    s.record_visit(ws, "https://v.example/", "Keep").unwrap();
    s.update_visit_title(ws, "https://v.example/", "").unwrap();
    s.update_visit_title(ws, "https://missing.example/", "X")
        .unwrap();

    let rows = ws_rows(&s, ws);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "Keep");
}

#[test]
fn clear_scoped_to_a_workspace_leaves_others() {
    let s = store();
    let a = WorkspaceId::new();
    let b = WorkspaceId::new();
    s.record_visit(a, "https://a.example/", "A").unwrap();
    s.record_visit(b, "https://b.example/", "B").unwrap();

    s.clear_history(Some(a), None).unwrap();
    assert!(ws_rows(&s, a).is_empty(), "workspace a cleared");
    assert_eq!(ws_rows(&s, b).len(), 1, "workspace b untouched");
}

#[test]
fn clear_all_removes_everything() {
    let s = store();
    let a = WorkspaceId::new();
    let b = WorkspaceId::new();
    s.record_visit(a, "https://a.example/", "A").unwrap();
    s.record_visit(b, "https://b.example/", "B").unwrap();

    s.clear_history(None, None).unwrap();
    assert!(all_rows(&s).is_empty());
}

#[test]
fn clear_since_a_future_instant_keeps_everything() {
    let s = store();
    let a = WorkspaceId::new();
    s.record_visit(a, "https://a.example/", "A").unwrap();

    let future = chrono::Utc::now() + chrono::Duration::hours(1);
    s.clear_history(None, Some(future)).unwrap();

    assert_eq!(
        all_rows(&s).len(),
        1,
        "nothing was visited after a future instant"
    );
}

#[test]
fn clear_since_a_past_instant_removes_recent_visits() {
    let s = store();
    let a = WorkspaceId::new();
    s.record_visit(a, "https://a.example/", "A").unwrap();

    let past = chrono::Utc::now() - chrono::Duration::hours(1);
    s.clear_history(None, Some(past)).unwrap();

    assert!(all_rows(&s).is_empty());
}
