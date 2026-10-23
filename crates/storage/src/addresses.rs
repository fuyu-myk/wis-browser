//! Saved-address records: each address sealed whole as AEAD `ciphertext` +
//! `nonce` by the `wis-secrets` crate, so nothing about it is readable here.

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressRecord {
    pub id: i64,
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
