//! Tests for the completed-download history store.

use chrono::{Duration, Utc};
use wis_core::DownloadId;
use wis_storage::{DownloadRecord, SqliteStorage, Storage};

fn store() -> SqliteStorage {
    SqliteStorage::open_in_memory().expect("in-memory DB")
}

fn record(filename: &str, age_minutes: i64) -> DownloadRecord {
    DownloadRecord {
        id: DownloadId::new(),
        url: format!("https://example.com/{filename}"),
        filename: filename.to_string(),
        path: format!("/Users/someone/Downloads/{filename}"),
        bytes: 1024,
        completed_at: Utc::now() - Duration::minutes(age_minutes),
    }
}

#[test]
fn record_and_list_round_trips() {
    let s = store();
    let rec = record("report.pdf", 0);
    s.record_download(&rec).unwrap();

    let listed = s.list_downloads(10).unwrap();
    assert_eq!(listed, vec![rec]);
}

#[test]
fn list_is_newest_first_and_capped() {
    let s = store();
    s.record_download(&record("old.zip", 30)).unwrap();
    s.record_download(&record("mid.zip", 20)).unwrap();
    s.record_download(&record("new.zip", 10)).unwrap();

    let listed = s.list_downloads(2).unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].filename, "new.zip");
    assert_eq!(listed[1].filename, "mid.zip");
}

#[test]
fn record_is_idempotent_per_id() {
    let s = store();
    let mut rec = record("a.txt", 0);
    s.record_download(&rec).unwrap();
    rec.bytes = 2048;
    s.record_download(&rec).unwrap();

    let listed = s.list_downloads(10).unwrap();
    assert_eq!(listed.len(), 1, "same id must not duplicate");
    assert_eq!(listed[0].bytes, 2048);
}

#[test]
fn delete_removes_one_row_and_ignores_unknown_ids() {
    let s = store();
    let keep = record("keep.txt", 0);
    let drop = record("drop.txt", 1);
    s.record_download(&keep).unwrap();
    s.record_download(&drop).unwrap();

    s.delete_download(drop.id).unwrap();
    s.delete_download(DownloadId::new()).unwrap();

    let listed = s.list_downloads(10).unwrap();
    assert_eq!(listed, vec![keep]);
}

#[test]
fn clear_empties_the_history() {
    let s = store();
    s.record_download(&record("a.txt", 0)).unwrap();
    s.record_download(&record("b.txt", 1)).unwrap();

    s.clear_downloads().unwrap();
    assert!(s.list_downloads(10).unwrap().is_empty());
}
