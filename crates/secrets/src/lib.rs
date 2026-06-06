//! Password-vault cryptography and master-key storage.
//!
//! - **Crypto:** entries are sealed with ChaCha20-Poly1305 (AEAD) under a
//!   32-byte master key, with a fresh random nonce per seal.
//! - **Master key:** generated once with the OS CSPRNG and kept in the macOS
//!   login Keychain (this-device-only). It is released into memory only after a
//!   successful Touch ID check (enforced by the shell via `LocalAuthentication`
//!   before calling [`keychain_load`]); the in-memory copy zeroizes on drop.
//!
//! Plaintext passwords exist only transiently in memory during seal/open and
//! are never logged or persisted.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::RngCore;
use zeroize::Zeroizing;

/// A 32-byte vault master key; zeroized on drop.
pub type MasterKey = Zeroizing<[u8; 32]>;

/// A sealed value: AEAD ciphertext plus the random nonce used to seal it.
#[derive(Debug, Clone)]
pub struct EncryptedBlob {
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("crypto failure")]
    Crypto,
    #[error("malformed sealed data")]
    Malformed,
    #[error("keychain: {0}")]
    Keychain(String),
    #[error("vault is unavailable on this platform")]
    Unsupported,
}

/// Generate a fresh master key from the OS CSPRNG.
pub fn generate_master_key() -> MasterKey {
    let mut key = Zeroizing::new([0u8; 32]);
    rand::rngs::OsRng.fill_bytes(key.as_mut_slice());
    key
}

/// Seal `plaintext` under `key` with a fresh random 96-bit nonce.
pub fn encrypt(key: &MasterKey, plaintext: &[u8]) -> Result<EncryptedBlob, VaultError> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_ref()));
    let mut nonce_bytes = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| VaultError::Crypto)?;

    Ok(EncryptedBlob {
        ciphertext,
        nonce: nonce_bytes.to_vec(),
    })
}

/// Open a sealed value. Returns the plaintext bytes (caller must handle them
/// carefully and avoid logging).
pub fn decrypt(key: &MasterKey, blob: &EncryptedBlob) -> Result<Vec<u8>, VaultError> {
    if blob.nonce.len() != 12 {
        return Err(VaultError::Malformed);
    }
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key.as_ref()));
    let nonce = Nonce::from_slice(&blob.nonce);

    cipher
        .decrypt(nonce, blob.ciphertext.as_ref())
        .map_err(|_| VaultError::Crypto)
}

// ---------------------------------------------------------------------------
// Master-key storage (macOS Keychain)
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
const KEYCHAIN_SERVICE: &str = "dev.wis.browser.vault";
#[cfg(target_os = "macos")]
const KEYCHAIN_ACCOUNT: &str = "master-key";

/// Load the master key from the Keychain, or `None` if the vault was never set
/// up. Callers must gate this behind a successful biometric check.
#[cfg(target_os = "macos")]
pub fn keychain_load() -> Result<Option<MasterKey>, VaultError> {
    use security_framework::passwords::get_generic_password;
    match get_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT) {
        Ok(bytes) => {
            if bytes.len() != 32 {
                return Err(VaultError::Malformed);
            }
            let mut key = Zeroizing::new([0u8; 32]);
            key.copy_from_slice(&bytes);
            Ok(Some(key))
        }
        Err(err) if err.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
        Err(err) => Err(VaultError::Keychain(err.to_string())),
    }
}

/// errSecItemNotFound.
#[cfg(target_os = "macos")]
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;

/// Persist the master key in the Keychain (this-device login keychain).
#[cfg(target_os = "macos")]
pub fn keychain_store(key: &MasterKey) -> Result<(), VaultError> {
    use security_framework::passwords::{delete_generic_password, set_generic_password};

    let _ = delete_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT);
    set_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, key.as_ref())
        .map_err(|err| VaultError::Keychain(err.to_string()))
}

/// Remove the master key (renders all stored credentials undecryptable).
#[cfg(target_os = "macos")]
pub fn keychain_delete() -> Result<(), VaultError> {
    use security_framework::passwords::delete_generic_password;
    match delete_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT) {
        Ok(()) => Ok(()),
        Err(err) if err.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
        Err(err) => Err(VaultError::Keychain(err.to_string())),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn keychain_load() -> Result<Option<MasterKey>, VaultError> {
    Err(VaultError::Unsupported)
}
#[cfg(not(target_os = "macos"))]
pub fn keychain_store(_key: &MasterKey) -> Result<(), VaultError> {
    Err(VaultError::Unsupported)
}
#[cfg(not(target_os = "macos"))]
pub fn keychain_delete() -> Result<(), VaultError> {
    Err(VaultError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_round_trips() {
        let key = generate_master_key();
        let blob = encrypt(&key, b"hunter2").unwrap();
        assert_ne!(blob.ciphertext, b"hunter2");
        assert_eq!(blob.nonce.len(), 12);
        assert_eq!(decrypt(&key, &blob).unwrap(), b"hunter2");
    }

    #[test]
    fn wrong_key_fails_to_open() {
        let blob = encrypt(&generate_master_key(), b"secret").unwrap();
        assert!(decrypt(&generate_master_key(), &blob).is_err());
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let key = generate_master_key();
        let mut blob = encrypt(&key, b"secret").unwrap();
        blob.ciphertext[0] ^= 0xff;
        assert!(decrypt(&key, &blob).is_err());
    }

    #[test]
    fn each_seal_uses_a_fresh_nonce() {
        let key = generate_master_key();
        let a = encrypt(&key, b"same").unwrap();
        let b = encrypt(&key, b"same").unwrap();
        assert_ne!(a.nonce, b.nonce);
        assert_ne!(a.ciphertext, b.ciphertext);
    }
}
