//! Recently-closed tabs: recording, browsing, reopening, and the two ways
//! entries leave the list (a cap, and clearing browsing history).

use chrono::{Duration, Utc};
use wis_core::WorkspaceId;
use wis_storage::{ClosedTabRecord, SqliteStorage, Storage};

fn store() -> SqliteStorage {
    SqliteStorage::open_in_memory().expect("in-memory DB")
}

/// A record whose id the store will replace.
fn entry(workspace: WorkspaceId, url: &str, minutes_ago: i64) -> ClosedTabRecord {
    ClosedTabRecord {
        id: 0,
        workspace_id: workspace,
        url: url.to_string(),
        title: format!("Title for {url}"),
        folder_id: None,
        position: 3,
        pinned: false,
        closed_at: Utc::now() - Duration::minutes(minutes_ago),
    }
}

#[test]
fn closed_tabs_come_back_newest_first() {
    let s = store();
    let ws = WorkspaceId::new();
    s.record_closed_tab(&entry(ws, "https://old.example/", 10))
        .unwrap();
    s.record_closed_tab(&entry(ws, "https://new.example/", 1))
        .unwrap();

    let urls: Vec<String> = s
        .list_closed_tabs(10)
        .unwrap()
        .into_iter()
        .map(|e| e.url)
        .collect();
    assert_eq!(urls, vec!["https://new.example/", "https://old.example/"]);
}

#[test]
fn recording_hands_back_the_row_id_and_keeps_the_fields() {
    let s = store();
    let ws = WorkspaceId::new();
    let mut written = entry(ws, "https://a.example/", 1);
    written.pinned = true;
    written.position = 7;

    let id = s.record_closed_tab(&written).unwrap();
    assert!(id > 0, "the store assigns the id");

    let stored = s.list_closed_tabs(10).unwrap().remove(0);
    assert_eq!(stored.id, id);
    assert_eq!(stored.workspace_id, ws);
    assert_eq!(stored.url, written.url);
    assert_eq!(stored.title, written.title);
    assert_eq!(stored.position, 7);
    assert!(stored.pinned);
}

#[test]
fn taking_one_reopens_it_and_removes_it_from_the_list() {
    let s = store();
    let ws = WorkspaceId::new();
    s.record_closed_tab(&entry(ws, "https://a.example/", 5))
        .unwrap();
    let wanted = s
        .record_closed_tab(&entry(ws, "https://b.example/", 1))
        .unwrap();

    let taken = s.take_closed_tab(wanted).unwrap().expect("the entry");
    assert_eq!(taken.url, "https://b.example/");

    let left: Vec<String> = s
        .list_closed_tabs(10)
        .unwrap()
        .into_iter()
        .map(|e| e.url)
        .collect();
    assert_eq!(left, vec!["https://a.example/"], "a reopened tab is gone");
    assert!(
        s.take_closed_tab(wanted).unwrap().is_none(),
        "and cannot be taken twice"
    );
}

#[test]
fn taking_the_latest_walks_backwards_through_the_list() {
    let s = store();
    let ws = WorkspaceId::new();
    s.record_closed_tab(&entry(ws, "https://first.example/", 9))
        .unwrap();
    s.record_closed_tab(&entry(ws, "https://second.example/", 5))
        .unwrap();

    assert_eq!(
        s.take_latest_closed_tab().unwrap().unwrap().url,
        "https://second.example/"
    );
    assert_eq!(
        s.take_latest_closed_tab().unwrap().unwrap().url,
        "https://first.example/"
    );
    assert!(s.take_latest_closed_tab().unwrap().is_none());
}

#[test]
fn pruning_keeps_only_the_newest() {
    let s = store();
    let ws = WorkspaceId::new();
    for minutes in (1..=5).rev() {
        s.record_closed_tab(&entry(ws, &format!("https://{minutes}.example/"), minutes))
            .unwrap();
    }

    s.prune_closed_tabs(2).unwrap();

    let urls: Vec<String> = s
        .list_closed_tabs(10)
        .unwrap()
        .into_iter()
        .map(|e| e.url)
        .collect();
    assert_eq!(urls, vec!["https://1.example/", "https://2.example/"]);
}

/// An archived tab is stamped with when it was last used, so the cap must
/// follow recording order or it would be dropped on arrival.
#[test]
fn pruning_keeps_the_last_recorded_however_long_ago_they_were_used() {
    let s = store();
    let ws = WorkspaceId::new();
    for minutes in (1..=3).rev() {
        s.record_closed_tab(&entry(ws, &format!("https://{minutes}.example/"), minutes))
            .unwrap();
    }
    s.record_closed_tabs(&[entry(ws, "https://archived.example/", 8 * 24 * 60)])
        .unwrap();

    s.prune_closed_tabs(3).unwrap();

    let urls: Vec<String> = s
        .list_closed_tabs(10)
        .unwrap()
        .into_iter()
        .map(|e| e.url)
        .collect();
    assert_eq!(
        urls,
        vec![
            "https://1.example/",
            "https://2.example/",
            "https://archived.example/"
        ]
    );
}

#[test]
fn several_closed_tabs_record_together() {
    let s = store();
    let ws = WorkspaceId::new();
    let ids = s
        .record_closed_tabs(&[
            entry(ws, "https://a.example/", 2),
            entry(ws, "https://b.example/", 1),
        ])
        .unwrap();

    assert_eq!(ids.len(), 2);
    assert_eq!(s.list_closed_tabs(10).unwrap().len(), 2);
}

#[test]
fn clearing_scopes_to_a_workspace() {
    let s = store();
    let a = WorkspaceId::new();
    let b = WorkspaceId::new();
    s.record_closed_tab(&entry(a, "https://a.example/", 1))
        .unwrap();
    s.record_closed_tab(&entry(b, "https://b.example/", 1))
        .unwrap();

    s.clear_closed_tabs(Some(a), None).unwrap();

    let urls: Vec<String> = s
        .list_closed_tabs(10)
        .unwrap()
        .into_iter()
        .map(|e| e.url)
        .collect();
    assert_eq!(urls, vec!["https://b.example/"]);
}

#[test]
fn clearing_since_an_instant_keeps_older_entries() {
    let s = store();
    let ws = WorkspaceId::new();
    s.record_closed_tab(&entry(ws, "https://old.example/", 120))
        .unwrap();
    s.record_closed_tab(&entry(ws, "https://new.example/", 1))
        .unwrap();

    s.clear_closed_tabs(None, Some(Utc::now() - Duration::minutes(30)))
        .unwrap();

    let urls: Vec<String> = s
        .list_closed_tabs(10)
        .unwrap()
        .into_iter()
        .map(|e| e.url)
        .collect();
    assert_eq!(urls, vec!["https://old.example/"]);
}

#[test]
fn deleting_a_page_from_history_takes_its_closed_tab_with_it() {
    let s = store();
    let ws = WorkspaceId::new();
    s.record_visit(ws, "https://gone.example/", "Gone").unwrap();
    s.record_visit(ws, "https://stays.example/", "Stays")
        .unwrap();
    s.record_closed_tab(&entry(ws, "https://gone.example/", 1))
        .unwrap();
    s.record_closed_tab(&entry(ws, "https://stays.example/", 1))
        .unwrap();

    let doomed: Vec<i64> = s
        .query_history(&wis_storage::HistoryQuery::default())
        .unwrap()
        .into_iter()
        .filter(|row| row.url == "https://gone.example/")
        .map(|row| row.id)
        .collect();
    let urls = s.history_urls(&doomed).unwrap();
    assert_eq!(urls, vec!["https://gone.example/"]);

    s.delete_closed_tabs_by_urls(&urls).unwrap();

    let left: Vec<String> = s
        .list_closed_tabs(10)
        .unwrap()
        .into_iter()
        .map(|e| e.url)
        .collect();
    assert_eq!(left, vec!["https://stays.example/"]);
}
