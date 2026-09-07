//! Password-vault cryptography and master-key storage.
//!
//! - **Crypto:** entries are sealed with ChaCha20-Poly1305 (AEAD) under a
//!   32-byte master key, with a fresh random nonce per seal.
//! - **Master key:** generated once with the OS CSPRNG and kept in the OS
//!   secure store, this-device-only: the macOS login Keychain, or on Windows a
//!   DPAPI-sealed blob in the per-user app data folder. It is released into
//!   memory only after a successful biometric check (Touch ID via
//!   `LocalAuthentication`, or Windows Hello via `UserConsentVerifier`,
//!   enforced by the shell before calling [`keychain_load`]); the in-memory
//!   copy zeroizes on drop.
//! - **AI provider key:** A variable-length UTF-8 API key, stored under its own
//!   dedicated account, distinct from the vault master key and the session-cookie
//!   key. Like the session-cookie key, it is not biometric-gated. Returns
//!   `Zeroizing<Vec<u8>>` because the length is arbitrary.
//!
//! The `keychain_*` names are the cross-platform key-store surface; on Windows
//! they are DPAPI-on-disk, not a literal keychain.
//!
//! Plaintext passwords exist only transiently in memory during seal/open and
//! are never logged or persisted.

pub mod breach;
pub mod password;

pub use breach::{count_in_range, range_split, RANGE_PREFIX_LEN};
pub use password::{
    generate_password, PasswordRecipe, DEFAULT_PASSWORD_LENGTH, MAX_PASSWORD_LENGTH,
    MIN_PASSWORD_LENGTH,
};

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

// ---------------------------------------------------------------------------
// BYOK AI provider key storage
// ---------------------------------------------------------------------------

/// macOS: login Keychain generic password. Account distinct from
/// `master-key` and `session-cookie-key`.
#[cfg(target_os = "macos")]
const KEYCHAIN_AI_KEY_ACCOUNT: &str = "ai-provider-key";

/// Store (overwriting any existing) the provider API key. Not biometric-gated.
#[cfg(target_os = "macos")]
pub fn ai_key_store(secret: &[u8]) -> Result<(), VaultError> {
    use security_framework::passwords::{delete_generic_password, set_generic_password};

    let _ = delete_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_AI_KEY_ACCOUNT);
    set_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_AI_KEY_ACCOUNT, secret)
        .map_err(|e| VaultError::Keychain(e.to_string()))
}

/// Load the stored provider API key, or `None` when never set. Not
/// biometric-gated.
#[cfg(target_os = "macos")]
pub fn ai_key_load() -> Result<Option<Zeroizing<Vec<u8>>>, VaultError> {
    use security_framework::passwords::get_generic_password;

    match get_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_AI_KEY_ACCOUNT) {
        Ok(bytes) => Ok(Some(Zeroizing::new(bytes))),
        Err(err) if err.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(None),
        Err(err) => Err(VaultError::Keychain(err.to_string())),
    }
}

/// Delete the stored provider API key (idempotent).
#[cfg(target_os = "macos")]
pub fn ai_key_delete() -> Result<(), VaultError> {
    use security_framework::passwords::delete_generic_password;

    match delete_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_AI_KEY_ACCOUNT) {
        Ok(()) => Ok(()),
        Err(err) if err.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
        Err(err) => Err(VaultError::Keychain(err.to_string())),
    }
}

// ---------------------------------------------------------------------------
// Master-key storage (Windows DPAPI)
// ---------------------------------------------------------------------------
//
// DPAPI only encrypts; unlike the Keychain it provides no storage, so the
// sealed blob is written to a file in the per-user app data folder (beside the
// SQLite profile under the bundle identifier). DPAPI binds the blob to the
// Windows user account, so the on-disk file is useless to another account or a
// stolen disk (the user-scope analogue of the Keychain ACL and Chrome's
// DPAPI-protected "Safe Storage" key). Biometric gating is a separate UI layer
// in the shell (`UserConsentVerifier`), exactly as Touch ID gates the Keychain.

#[cfg(windows)]
const WIN_APP_DIR: &str = "app.myk.wis";
#[cfg(windows)]
const MASTER_KEY_FILE: &str = "vault-master.key";
#[cfg(windows)]
const SESSION_KEY_FILE: &str = "session-cookie.key";
#[cfg(windows)]
const AI_KEY_FILE: &str = "ai-provider.key";

/// The per-user app data folder holding the sealed key blobs, created on first
/// use. Matches Tauri's `app_data_dir()` so the keys sit beside `wis.db`.
#[cfg(windows)]
fn secrets_dir() -> Result<std::path::PathBuf, VaultError> {
    let base = dirs::data_dir().ok_or_else(|| VaultError::Keychain("no app data dir".into()))?;
    let dir = base.join(WIN_APP_DIR);
    std::fs::create_dir_all(&dir)
        .map_err(|err| VaultError::Keychain(format!("create secrets dir: {err}")))?;

    Ok(dir)
}

/// Seal `plain` with DPAPI (user scope, no UI). The returned ciphertext is safe
/// to write to disk; only this Windows user can unseal it.
#[cfg(windows)]
fn dpapi_protect(plain: &[u8]) -> Result<Vec<u8>, VaultError> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let in_blob = CRYPT_INTEGER_BLOB {
        cbData: plain.len() as u32,
        pbData: plain.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };

    let ok = unsafe {
        CryptProtectData(
            &in_blob,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out_blob,
        )
    };
    if ok == 0 {
        return Err(VaultError::Keychain("DPAPI protect failed".into()));
    }

    // Copy the OS-owned buffer out, then release it with LocalFree.
    let sealed =
        unsafe { std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec() };
    unsafe { LocalFree(out_blob.pbData as *mut core::ffi::c_void) };

    Ok(sealed)
}

/// Unseal a DPAPI blob. The plaintext is wrapped in `Zeroizing` so it never
/// lingers in memory.
#[cfg(windows)]
fn dpapi_unprotect(sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let in_blob = CRYPT_INTEGER_BLOB {
        cbData: sealed.len() as u32,
        pbData: sealed.as_ptr() as *mut u8,
    };
    let mut out_blob = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };

    let ok = unsafe {
        CryptUnprotectData(
            &in_blob,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out_blob,
        )
    };
    if ok == 0 {
        return Err(VaultError::Keychain("DPAPI unprotect failed".into()));
    }

    let plain = Zeroizing::new(unsafe {
        std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec()
    });
    unsafe { LocalFree(out_blob.pbData as *mut core::ffi::c_void) };

    Ok(plain)
}

/// Unseal a file's DPAPI blob into a validated 32-byte master key.
#[cfg(windows)]
fn master_key_from_sealed(sealed: &[u8]) -> Result<MasterKey, VaultError> {
    let plain = dpapi_unprotect(sealed)?;
    if plain.len() != 32 {
        return Err(VaultError::Malformed);
    }
    let mut key = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(&plain);

    Ok(key)
}

#[cfg(windows)]
pub fn keychain_load() -> Result<Option<MasterKey>, VaultError> {
    let path = secrets_dir()?.join(MASTER_KEY_FILE);
    match std::fs::read(&path) {
        Ok(sealed) => Ok(Some(master_key_from_sealed(&sealed)?)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(VaultError::Keychain(format!("read master key: {err}"))),
    }
}

#[cfg(windows)]
pub fn keychain_store(key: &MasterKey) -> Result<(), VaultError> {
    let sealed = dpapi_protect(key.as_ref())?;
    let path = secrets_dir()?.join(MASTER_KEY_FILE);
    std::fs::write(&path, &sealed)
        .map_err(|err| VaultError::Keychain(format!("write master key: {err}")))
}

#[cfg(windows)]
pub fn keychain_delete() -> Result<(), VaultError> {
    let path = secrets_dir()?.join(MASTER_KEY_FILE);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(VaultError::Keychain(format!("delete master key: {err}"))),
    }
}

/// Load (creating on first use) the session-cookie key. Like macOS this is
/// **not** biometric-gated: it must be available automatically at launch.
#[cfg(windows)]
pub fn session_key_load_or_create() -> Result<MasterKey, VaultError> {
    let path = secrets_dir()?.join(SESSION_KEY_FILE);
    match std::fs::read(&path) {
        Ok(sealed) => master_key_from_sealed(&sealed),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let key = generate_master_key();
            let sealed = dpapi_protect(key.as_ref())?;
            std::fs::write(&path, &sealed)
                .map_err(|e| VaultError::Keychain(format!("write session key: {e}")))?;
            Ok(key)
        }
        Err(err) => Err(VaultError::Keychain(format!("read session key: {err}"))),
    }
}

/// Windows: DPAPI-sealed file beside the vault + session keys. Not
/// biometric-gated.
#[cfg(windows)]
pub fn ai_key_store(secret: &[u8]) -> Result<(), VaultError> {
    let sealed = dpapi_protect(secret)?;
    let path = secrets_dir()?.join(AI_KEY_FILE);
    std::fs::write(&path, &sealed).map_err(|e| VaultError::Keychain(format!("write ai key: {e}")))
}

#[cfg(windows)]
pub fn ai_key_load() -> Result<Option<Zeroizing<Vec<u8>>>, VaultError> {
    let path = secrets_dir()?.join(AI_KEY_FILE);
    match std::fs::read(&path) {
        Ok(sealed) => Ok(Some(dpapi_unprotect(&sealed)?)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(VaultError::Keychain(format!("read ai key: {err}"))),
    }
}

#[cfg(windows)]
pub fn ai_key_delete() -> Result<(), VaultError> {
    let path = secrets_dir()?.join(AI_KEY_FILE);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(VaultError::Keychain(format!("delete ai key: {err}"))),
    }
}

// ---------------------------------------------------------------------------
// Master-key storage (other platforms)
// ---------------------------------------------------------------------------

#[cfg(not(any(target_os = "macos", windows)))]
pub fn ai_key_store(_secret: &[u8]) -> Result<(), VaultError> {
    Err(VaultError::Unsupported)
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn ai_key_load() -> Result<Option<Zeroizing<Vec<u8>>>, VaultError> {
    Err(VaultError::Unsupported)
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn ai_key_delete() -> Result<(), VaultError> {
    Err(VaultError::Unsupported)
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn keychain_load() -> Result<Option<MasterKey>, VaultError> {
    Err(VaultError::Unsupported)
}
#[cfg(not(any(target_os = "macos", windows)))]
pub fn keychain_store(_key: &MasterKey) -> Result<(), VaultError> {
    Err(VaultError::Unsupported)
}
#[cfg(not(any(target_os = "macos", windows)))]
pub fn keychain_delete() -> Result<(), VaultError> {
    Err(VaultError::Unsupported)
}
#[cfg(not(any(target_os = "macos", windows)))]
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
