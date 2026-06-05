//! Tests for the password-vault credential store (ciphertext is opaque here).

use wis_storage::{SqliteStorage, Storage};

fn store() -> SqliteStorage {
    SqliteStorage::open_in_memory().expect("in-memory DB")
}

#[test]
fn save_then_list_and_get() {
    let s = store();
    let id = s
        .save_credential("github.com", "alice", b"CIPHER", b"NONCE12bytes")
        .unwrap();

    let all = s.list_credentials(None).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].host, "github.com");
    assert_eq!(all[0].username, "alice");
    assert_eq!(all[0].ciphertext, b"CIPHER");
    assert_eq!(all[0].nonce, b"NONCE12bytes");

    let one = s.get_credential(id).unwrap().unwrap();
    assert_eq!(one.id, id);
    assert_eq!(one.ciphertext, b"CIPHER");
}

#[test]
fn saving_same_host_user_updates_in_place() {
    let s = store();
    let id1 = s.save_credential("example.com", "bob", b"old", b"n1").unwrap();
    let id2 = s.save_credential("example.com", "bob", b"new", b"n2").unwrap();
    assert_eq!(id1, id2, "same (host, username) updates the same row");

    let all = s.list_credentials(None).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].ciphertext, b"new");
    assert_eq!(all[0].nonce, b"n2");
}

#[test]
fn list_filters_by_host() {
    let s = store();
    s.save_credential("a.example", "u", b"c", b"n").unwrap();
    s.save_credential("b.example", "u", b"c", b"n").unwrap();

    let only_a = s.list_credentials(Some("a.example")).unwrap();
    assert_eq!(only_a.len(), 1);
    assert_eq!(only_a[0].host, "a.example");
}

#[test]
fn multiple_users_per_host_are_distinct() {
    let s = store();
    s.save_credential("site.com", "alice", b"c", b"n").unwrap();
    s.save_credential("site.com", "bob", b"c", b"n").unwrap();
    assert_eq!(s.list_credentials(Some("site.com")).unwrap().len(), 2);
}

#[test]
fn delete_removes_the_row() {
    let s = store();
    let id = s.save_credential("x.com", "u", b"c", b"n").unwrap();
    s.delete_credential(id).unwrap();
    assert!(s.get_credential(id).unwrap().is_none());
    assert!(s.list_credentials(None).unwrap().is_empty());
}
