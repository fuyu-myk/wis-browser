//! Saved payment cards. What a card shows without the vault key sits in the
//! clear (nickname, network, last four digits, expiry). The card itself,
//! number and name included, is sealed whole as AEAD `ciphertext` + `nonce`.

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardRecord {
    pub id: i64,
    pub nickname: String,
    pub brand: String,
    pub last4: String,
    pub exp_month: u32,
    pub exp_year: u32,
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A card to store: its plain summary and the sealed card.
#[derive(Debug, Clone, Copy)]
pub struct CardRow<'a> {
    pub nickname: &'a str,
    pub brand: &'a str,
    pub last4: &'a str,
    pub exp_month: u32,
    pub exp_year: u32,
    pub ciphertext: &'a [u8],
    pub nonce: &'a [u8],
}
