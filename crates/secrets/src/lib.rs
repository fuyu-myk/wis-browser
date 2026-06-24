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

use argon2::Argon2;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::RngCore;
use zeroize::Zeroizing;

/// A 32-byte vault master key; zeroized on drop.
pub type MasterKey = Zeroizing<[u8; 32]>;

/// Salt length (bytes) for passphrase key derivation.
pub const BACKUP_SALT_LEN: usize = 16;

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

/// A fresh random salt for passphrase key derivation.
pub fn random_salt() -> [u8; BACKUP_SALT_LEN] {
    let mut salt = [0u8; BACKUP_SALT_LEN];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    salt
}

/// Derive a 32-byte key from a passphrase and salt with Argon2id (default
/// params). Used to encrypt portable backups under a user-chosen passphrase,
/// deliberately distinct from the device-bound Keychain master key so a bundle
/// is decryptable on another machine. The same passphrase + salt always yields
/// the same key; a wrong passphrase yields a different key and AEAD open fails.
pub fn derive_passphrase_key(passphrase: &str, salt: &[u8]) -> Result<MasterKey, VaultError> {
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, key.as_mut_slice())
        .map_err(|_| VaultError::Crypto)?;

    Ok(key)
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

/// Keychain account for the session-cookie encryption key (distinct from the
/// vault master key).
#[cfg(target_os = "macos")]
const KEYCHAIN_COOKIE_ACCOUNT: &str = "session-cookie-key";

/// Load (creating it on first use) the symmetric key that encrypts persisted
/// session cookies. Unlike the vault master key this is deliberately **not**
/// gated by Touch ID: it must be available automatically at launch, since
/// session cookies are restored before any user interaction. It lives in the
/// login Keychain ACL'd to this app, so the on-disk blob is useless without
/// Keychain access (mirrors Chrome's "Safe Storage" key). 256-bit, OS CSPRNG.
#[cfg(target_os = "macos")]
pub fn session_key_load_or_create() -> Result<MasterKey, VaultError> {
    use security_framework::passwords::{get_generic_password, set_generic_password};

    match get_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_COOKIE_ACCOUNT) {
        Ok(bytes) => {
            if bytes.len() != 32 {
                return Err(VaultError::Malformed);
            }
            let mut key = Zeroizing::new([0u8; 32]);
            key.copy_from_slice(&bytes);
            Ok(key)
        }
        Err(err) if err.code() == ERR_SEC_ITEM_NOT_FOUND => {
            let key = generate_master_key();
            set_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_COOKIE_ACCOUNT, key.as_ref())
                .map_err(|e| VaultError::Keychain(e.to_string()))?;
            Ok(key)
        }
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
#[cfg(not(target_os = "macos"))]
pub fn session_key_load_or_create() -> Result<MasterKey, VaultError> {
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

    #[test]
    fn passphrase_key_is_deterministic_per_salt() {
        let salt = random_salt();
        let a = derive_passphrase_key("correct horse", &salt).unwrap();
        let b = derive_passphrase_key("correct horse", &salt).unwrap();
        assert_eq!(a.as_ref(), b.as_ref());
        // A different passphrase or salt diverges.
        let other_pass = derive_passphrase_key("wrong horse", &salt).unwrap();
        assert_ne!(a.as_ref(), other_pass.as_ref());
        let other_salt = derive_passphrase_key("correct horse", &random_salt()).unwrap();
        assert_ne!(a.as_ref(), other_salt.as_ref());
    }

    #[test]
    fn passphrase_round_trip_and_wrong_passphrase_fails() {
        let salt = random_salt();
        let key = derive_passphrase_key("s3cret", &salt).unwrap();
        let blob = encrypt(&key, b"backup payload").unwrap();

        // Re-deriving from the same passphrase + salt opens the bundle.
        let rederived = derive_passphrase_key("s3cret", &salt).unwrap();
        assert_eq!(decrypt(&rederived, &blob).unwrap(), b"backup payload");

        // A wrong passphrase derives a different key and fails to open.
        let wrong = derive_passphrase_key("nope", &salt).unwrap();
        assert!(decrypt(&wrong, &blob).is_err());
    }
}
