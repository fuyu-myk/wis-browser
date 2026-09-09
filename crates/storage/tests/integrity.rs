//! `is_intact` must tell a damaged profile apart from a healthy one, because
//! the shell uses it to decide whether a profile may be rotated away.

use std::fs;
use std::path::PathBuf;

use wis_storage::{is_intact, SqliteStorage};

fn temp_db_path(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("wis-integrity-test-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp db dir");
    dir.join("wis.db")
}

#[test]
fn healthy_database_is_intact() {
    let path = temp_db_path("healthy");
    let storage = SqliteStorage::open(&path).expect("open DB");
    drop(storage);

    assert!(is_intact(&path));
}

#[test]
fn garbled_file_is_not_intact() {
    let path = temp_db_path("garbled");
    let storage = SqliteStorage::open(&path).expect("open DB");
    drop(storage);

    // Overwrite the header so SQLite no longer recognises the file.
    let mut bytes = fs::read(&path).expect("read DB");
    bytes[0..16].copy_from_slice(b"not-a-sqlite-db!");
    fs::write(&path, &bytes).expect("write garbled DB");

    assert!(!is_intact(&path));
}

#[test]
fn missing_file_is_not_intact() {
    let path = temp_db_path("missing").with_file_name("does-not-exist.db");

    assert!(!is_intact(&path));
}
