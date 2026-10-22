//! The vault's master key sealed with a key derived from a password the user
//! types, for platforms without an OS key store and biometric gate.

use std::io::{Read, Write};
use std::path::Path;

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::RngCore;
use zeroize::Zeroizing;

use crate::{MasterKey, VaultError};

pub const MIN_PASSWORD_CHARS: usize = 12;
const MAGIC: &[u8; 8] = b"WISVPW01";
const SALT_LEN: usize = 16;
const HEADER_LEN: usize = MAGIC.len() + 12 + SALT_LEN;
const NONCE_LEN: usize = 12;
const SEALED_LEN: usize = HEADER_LEN + NONCE_LEN + 32 + 16;
pub const FILE_NAME: &str = "vault-master.sealed";

/// The Argon2id work a sealed file asks for, kept in its header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cost {
    memory_kib: u32,
    iterations: u32,
    lanes: u32,
}

impl Cost {
    pub const DEFAULT: Cost = Cost {
        memory_kib: 64 * 1024,
        iterations: 3,
        lanes: 1,
    };
    const MAX: Cost = Cost {
        memory_kib: 256 * 1024,
        iterations: 10,
        lanes: 4,
    };

    fn bounded(self) -> Option<Self> {
        let within = (1..=Self::MAX.memory_kib).contains(&self.memory_kib)
            && (1..=Self::MAX.iterations).contains(&self.iterations)
            && (1..=Self::MAX.lanes).contains(&self.lanes);
        within.then_some(self)
    }
}

/// `key` sealed with `password` at the default cost.
pub fn seal(key: &MasterKey, password: &str) -> Result<Vec<u8>, VaultError> {
    seal_with(key, password, Cost::DEFAULT)
}

fn seal_with(key: &MasterKey, password: &str, cost: Cost) -> Result<Vec<u8>, VaultError> {
    let mut salt = [0u8; SALT_LEN];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    let mut nonce = [0u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce);

    let header = header(&cost, &salt);
    let wrapping = derive(password, &salt, cost)?;
    let ciphertext = ChaCha20Poly1305::new(Key::from_slice(wrapping.as_slice()))
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: key.as_slice(),
                aad: &header,
            },
        )
        .map_err(|_| VaultError::Crypto)?;
    let mut sealed = Vec::with_capacity(SEALED_LEN);
    sealed.extend_from_slice(&header);
    sealed.extend_from_slice(&nonce);
    sealed.extend_from_slice(&ciphertext);

    Ok(sealed)
}

/// The master key in `sealed`, opened with `password`. A file asking for more
/// work than [`Cost::MAX`] is refused before any.
pub fn open(sealed: &[u8], password: &str) -> Result<MasterKey, VaultError> {
    if sealed.len() != SEALED_LEN || !sealed.starts_with(MAGIC) {
        return Err(VaultError::Malformed);
    }

    let cost = read_cost(sealed).ok_or(VaultError::Malformed)?;
    let (header, rest) = sealed.split_at(HEADER_LEN);
    let (nonce, ciphertext) = rest.split_at(NONCE_LEN);
    let wrapping = derive(password, &header[HEADER_LEN - SALT_LEN..], cost)?;
    let plaintext = Zeroizing::new(
        ChaCha20Poly1305::new(Key::from_slice(wrapping.as_slice()))
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: header,
                },
            )
            .map_err(|_| VaultError::WrongPassword)?,
    );
    if plaintext.len() != 32 {
        return Err(VaultError::Malformed);
    }

    let mut key = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(&plaintext);
    Ok(key)
}

fn header(cost: &Cost, salt: &[u8; SALT_LEN]) -> [u8; HEADER_LEN] {
    let mut header = [0u8; HEADER_LEN];
    header[..8].copy_from_slice(MAGIC);
    header[8..12].copy_from_slice(&cost.memory_kib.to_be_bytes());
    header[12..16].copy_from_slice(&cost.iterations.to_be_bytes());
    header[16..20].copy_from_slice(&cost.lanes.to_be_bytes());
    header[20..].copy_from_slice(salt);
    header
}

fn read_cost(header: &[u8]) -> Option<Cost> {
    let field = |at: usize| Some(u32::from_be_bytes(header.get(at..at + 4)?.try_into().ok()?));
    Cost {
        memory_kib: field(8)?,
        iterations: field(12)?,
        lanes: field(16)?,
    }
    .bounded()
}

fn derive(password: &str, salt: &[u8], cost: Cost) -> Result<MasterKey, VaultError> {
    let params = Params::new(cost.memory_kib, cost.iterations, cost.lanes, Some(32))
        .map_err(|_| VaultError::Malformed)?;
    let normalized = Zeroizing::new(
        icu_normalizer::ComposingNormalizerBorrowed::new_nfc()
            .normalize(password)
            .into_owned(),
    );
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(normalized.as_bytes(), salt, key.as_mut_slice())
        .map_err(|_| VaultError::Crypto)?;
    Ok(key)
}

pub fn read_file(path: &Path) -> Result<Option<Vec<u8>>, VaultError> {
    match std::fs::symlink_metadata(path) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(io_err("read sealed key", err)),
        Ok(meta) if !meta.is_file() => {
            return Err(VaultError::Io("the sealed key is not a file".into()))
        }
        Ok(_) => {}
    }
    let file = std::fs::File::open(path).map_err(|err| io_err("read sealed key", err))?;
    let mut bytes = Vec::with_capacity(SEALED_LEN + 1);
    file.take(SEALED_LEN as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| io_err("read sealed key", err))?;
    Ok(Some(bytes))
}

/// Replace the file at `path` with `bytes`, readable by the user alone.
pub fn write_file(path: &Path, bytes: &[u8]) -> Result<(), VaultError> {
    let dir = path
        .parent()
        .ok_or_else(|| VaultError::Io("sealed key has no folder".into()))?;
    std::fs::create_dir_all(dir).map_err(|err| io_err("write sealed key", err))?;
    let staged = dir.join(format!(
        ".{FILE_NAME}.{:016x}",
        rand::rngs::OsRng.next_u64()
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let written = options
        .open(&staged)
        .and_then(|mut file| file.write_all(bytes).and_then(|()| file.sync_all()))
        .and_then(|()| std::fs::rename(&staged, path));
    if let Err(err) = written {
        match std::fs::remove_file(&staged) {
            Err(cleanup) if cleanup.kind() != std::io::ErrorKind::NotFound => {
                log::warn!("vault: could not remove a staged sealed key: {cleanup}");
            }
            _ => {}
        }
        return Err(io_err("write sealed key", err));
    }

    #[cfg(unix)]
    if let Err(err) = std::fs::File::open(dir).and_then(|folder| folder.sync_all()) {
        log::warn!("vault: could not sync the sealed key's folder: {err}");
    }

    Ok(())
}

pub fn remove_file(path: &Path) -> Result<(), VaultError> {
    match std::fs::remove_file(path) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
            Err(io_err("remove sealed key", err))
        }
        _ => Ok(()),
    }
}

fn io_err(doing: &str, err: std::io::Error) -> VaultError {
    VaultError::Io(format!("{doing}: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate_master_key;

    fn quick() -> Cost {
        Cost {
            memory_kib: 64,
            iterations: 1,
            lanes: 1,
        }
    }

    #[test]
    fn a_sealed_key_opens_only_with_its_password() {
        let key = generate_master_key();
        let sealed = seal_with(&key, "correct horse", quick()).unwrap();

        assert_eq!(*open(&sealed, "correct horse").unwrap(), *key);
        assert!(matches!(
            open(&sealed, "wrong horse"),
            Err(VaultError::WrongPassword)
        ));
    }

    #[test]
    fn the_same_password_typed_two_unicode_ways_opens_the_vault() {
        let key = generate_master_key();
        let composed = "caf\u{e9} au lait";
        let decomposed = "cafe\u{301} au lait";
        let sealed = seal_with(&key, composed, quick()).unwrap();
        assert_eq!(*open(&sealed, decomposed).unwrap(), *key);
    }

    #[test]
    fn something_other_than_a_file_at_the_key_path_is_refused() {
        let dir = std::env::temp_dir().join(format!("wis-pv-dir-{}", std::process::id()));
        let path = dir.join("vault-master.sealed");
        std::fs::create_dir_all(&path).unwrap();
        assert!(matches!(read_file(&path), Err(VaultError::Io(_))));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_tampered_or_costly_file_is_refused_before_any_work() {
        let key = generate_master_key();
        let sealed = seal_with(&key, "correct horse", quick()).unwrap();

        let mut flipped = sealed.clone();
        *flipped.last_mut().unwrap() ^= 1;
        assert!(matches!(
            open(&flipped, "correct horse"),
            Err(VaultError::WrongPassword)
        ));

        let mut costly = sealed.clone();
        costly[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
        let started = std::time::Instant::now();
        assert!(matches!(
            open(&costly, "correct horse"),
            Err(VaultError::Malformed)
        ));
        assert!(started.elapsed() < std::time::Duration::from_secs(1));

        for broken in [&sealed[..20], &b"not a vault"[..], &[][..]] {
            assert!(matches!(
                open(broken, "correct horse"),
                Err(VaultError::Malformed)
            ));
        }
    }

    #[test]
    fn the_default_cost_is_written_into_the_file() {
        let header = header(&Cost::DEFAULT, &[0u8; 16]);
        assert_eq!(&header[..8], MAGIC);
        assert_eq!(read_cost(&header).unwrap(), Cost::DEFAULT);
    }

    #[test]
    fn the_file_is_replaced_whole_and_removed_cleanly() {
        let dir = std::env::temp_dir().join(format!("wis-pv-{}", std::process::id()));
        let path = dir.join("vault-master.sealed");
        assert_eq!(read_file(&path).unwrap(), None);

        write_file(&path, b"first").unwrap();
        write_file(&path, b"second").unwrap();
        assert_eq!(read_file(&path).unwrap().as_deref(), Some(&b"second"[..]));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        remove_file(&path).unwrap();
        remove_file(&path).unwrap();
        assert_eq!(read_file(&path).unwrap(), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
