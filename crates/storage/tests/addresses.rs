//! Tests for the sealed address store (ciphertext is opaque here).

use wis_storage::{SqliteStorage, Storage};

fn store() -> SqliteStorage {
    SqliteStorage::open_in_memory().expect("in-memory DB")
}

#[test]
fn a_saved_address_is_listed_and_replaced_in_place() {
    let s = store();
    assert!(!s.has_addresses().unwrap());

    let id = s.save_address(None, b"first", b"nonce-1").unwrap();
    let other = s.save_address(None, b"second", b"nonce-2").unwrap();
    assert_ne!(id, other);
    assert!(s.has_addresses().unwrap());

    assert_eq!(
        s.save_address(Some(id), b"changed", b"nonce-3").unwrap(),
        id
    );
    let rows = s.list_addresses().unwrap();
    assert_eq!(rows.len(), 2);
    let changed = rows.iter().find(|row| row.id == id).unwrap();
    assert_eq!(changed.ciphertext, b"changed");
    assert_eq!(changed.nonce, b"nonce-3");
    assert!(changed.updated_at >= changed.created_at);
}

#[test]
fn replacing_an_address_that_is_gone_is_an_error() {
    let s = store();
    assert!(s.save_address(Some(42), b"c", b"n").is_err());
}

#[test]
fn addresses_are_deleted_one_at_a_time_or_all_together() {
    let s = store();
    let id = s.save_address(None, b"a", b"n").unwrap();
    s.save_address(None, b"b", b"n").unwrap();

    s.delete_address(id).unwrap();
    assert_eq!(s.list_addresses().unwrap().len(), 1);
    s.clear_addresses().unwrap();
    assert!(s.list_addresses().unwrap().is_empty());
    assert!(!s.has_addresses().unwrap());
}

#[test]
fn an_id_is_never_handed_out_again_after_its_address_is_deleted() {
    let s = store();
    s.save_address(None, b"a", b"n").unwrap();
    let newest = s.save_address(None, b"b", b"n").unwrap();
    s.delete_address(newest).unwrap();
    s.delete_address(newest).unwrap();
    assert_ne!(s.save_address(None, b"c", b"n").unwrap(), newest);
}

#[test]
fn clearing_the_vault_removes_passwords_and_addresses_together() {
    let s = store();
    s.save_credential("x.com", "u", b"c", b"n").unwrap();
    s.save_address(None, b"a", b"n").unwrap();
    s.clear_vault().unwrap();
    assert!(!s.has_credentials().unwrap());
    assert!(!s.has_addresses().unwrap());
}
