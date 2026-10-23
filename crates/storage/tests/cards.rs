//! Tests for the card store: the summary is plain, the card itself opaque.

use wis_storage::{CardRow, SqliteStorage, Storage};

fn store() -> SqliteStorage {
    SqliteStorage::open_in_memory().expect("in-memory DB")
}

fn row<'a>(nickname: &'a str, sealed: &'a [u8]) -> CardRow<'a> {
    CardRow {
        nickname,
        brand: "visa",
        last4: "4242",
        exp_month: 8,
        exp_year: 2028,
        ciphertext: sealed,
        nonce: b"n",
    }
}

#[test]
fn a_saved_card_is_listed_and_replaced_in_place() {
    let s = store();
    assert!(!s.has_cards().unwrap());
    let id = s.save_card(None, &row("Travel", b"a")).unwrap();
    assert!(s.has_cards().unwrap());

    let listed = s.list_cards().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, id);
    assert_eq!(listed[0].nickname, "Travel");
    assert_eq!(listed[0].brand, "visa");
    assert_eq!(listed[0].last4, "4242");
    assert_eq!((listed[0].exp_month, listed[0].exp_year), (8, 2028));
    assert_eq!(listed[0].ciphertext, b"a");

    assert_eq!(s.save_card(Some(id), &row("Work", b"b")).unwrap(), id);
    let listed = s.list_cards().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].nickname, "Work");
    assert_eq!(listed[0].ciphertext, b"b");
}

#[test]
fn replacing_a_removed_card_fails() {
    let s = store();
    let id = s.save_card(None, &row("Travel", b"a")).unwrap();
    s.delete_card(id).unwrap();
    assert!(s.save_card(Some(id), &row("Travel", b"a")).is_err());
    assert!(!s.has_cards().unwrap());
}

#[test]
fn clearing_the_vault_removes_cards_too() {
    let s = store();
    s.save_credential("x.com", "u", b"c", b"n").unwrap();
    s.save_card(None, &row("Travel", b"a")).unwrap();
    s.clear_vault().unwrap();
    assert!(!s.has_credentials().unwrap());
    assert!(!s.has_cards().unwrap());
}
