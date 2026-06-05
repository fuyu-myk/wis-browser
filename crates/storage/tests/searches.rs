//! Tests for URL-bar search history + ranked site recommendations.

use wis_core::WorkspaceId;
use wis_storage::{SqliteStorage, Storage};

fn store() -> SqliteStorage {
    SqliteStorage::open_in_memory().expect("in-memory DB")
}

#[test]
fn record_and_suggest_searches() {
    let s = store();
    s.record_search("rust traits").unwrap();
    s.record_search("rust lifetimes").unwrap();
    s.record_search("weather").unwrap();

    let rust = s.suggest_searches("rust", 10).unwrap();
    assert_eq!(rust.len(), 2);
    assert!(rust.iter().all(|e| e.query.contains("rust")));
}

#[test]
fn repeat_search_bumps_count_and_ranks_first() {
    let s = store();
    s.record_search("alpha").unwrap();
    s.record_search("beta").unwrap();
    s.record_search("beta").unwrap();

    let all = s.suggest_searches("", 10).unwrap();
    assert_eq!(all[0].query, "beta", "most-used first");
    assert_eq!(all[0].count, 2);
}

#[test]
fn recent_searches_are_newest_first() {
    let s = store();
    s.record_search("first").unwrap();
    s.record_search("second").unwrap();
    s.record_search("third").unwrap();

    let recent = s.recent_searches(2).unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].query, "third", "newest first regardless of count");
    assert_eq!(recent[1].query, "second");
}

#[test]
fn delete_search_forgets_it() {
    let s = store();
    s.record_search("forget me").unwrap();
    s.delete_search("forget me").unwrap();
    assert!(s.suggest_searches("", 10).unwrap().is_empty());
}

#[test]
fn recommend_sites_ranks_by_visit_count() {
    let s = store();
    let ws = WorkspaceId::new();
    s.record_visit(ws, "https://rare.example/", "Rare").unwrap();
    for _ in 0..5 {
        s.record_visit(ws, "https://often.example/", "Often")
            .unwrap();
    }

    let recs = s.recommend_sites(ws, "", 10).unwrap();
    assert_eq!(recs[0].url, "https://often.example/", "most-visited first");
}

#[test]
fn recommend_sites_filters_by_query_and_workspace() {
    let s = store();
    let a = WorkspaceId::new();
    let b = WorkspaceId::new();
    s.record_visit(a, "https://github.com/", "GitHub").unwrap();
    s.record_visit(a, "https://gitlab.com/", "GitLab").unwrap();
    s.record_visit(b, "https://github.com/", "GitHub").unwrap();

    let recs = s.recommend_sites(a, "github", 10).unwrap();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].url, "https://github.com/");
}
