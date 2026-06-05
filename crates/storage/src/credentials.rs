//! Saved-password records.
//!
//! Each row stores a site `host` and `username` in the clear (so the list is
//! searchable without unlocking) plus the password sealed as AEAD `ciphertext`
//! + `nonce` (sealed by the `wis-secrets` crate). The decryption key never lives here.

use chrono::{DateTime, Utc};

/// One stored credential. `ciphertext`/`nonce` decrypt to the password only
/// with the vault master key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialRecord {
    pub id: i64,
    pub host: String,
    pub username: String,
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
