//! A copy of the vault's master key sealed to the user's own public key, in
//! the age format, so the private key can bring the vault back.

use std::fmt;
use std::io::{BufReader, Cursor, Read};
use std::str::FromStr;

pub use age::secrecy::SecretString;
use base64::Engine;
use zeroize::Zeroizing;

use crate::MasterKey;

pub const MAX_KEY_FILE_BYTES: usize = 64 * 1024;
pub const MAX_PUBLIC_KEY_BYTES: usize = 16 * 1024;
const MAX_SEALED_BYTES: usize = 16 * 1024;
const MAX_COMMENT_CHARS: usize = 100;
const MAX_BCRYPT_ROUNDS: u32 = 1024;
const MAX_SCRYPT_WORK_FACTOR: u8 = 20;

#[derive(Debug, thiserror::Error)]
pub enum RecoveryError {
    #[error("not an age or SSH public key")]
    NotAPublicKey,
    #[error("that is a private key; give the public key")]
    PrivateKeyGiven,
    #[error("not an age or SSH private key")]
    NotAPrivateKey,
    #[error("this key has a passphrase")]
    NeedsPassphrase,
    #[error("wrong passphrase")]
    WrongPassphrase,
    #[error("this key's passphrase protection asks for too much work")]
    ExcessiveWork,
    #[error("this key does not open the recovery copy")]
    WrongKey,
    #[error("the recovery copy is damaged")]
    Damaged,
    #[error("could not seal the recovery copy")]
    Sealing,
}

/// An age `age1...` key or an SSH ed25519 or RSA public key, with the SSH
/// key's comment.
pub struct Recipient {
    key: String,
    comment: Option<String>,
    kind: Kind,
}

enum Kind {
    Age(age::x25519::Recipient),
    Ssh(age::ssh::Recipient),
}

impl Recipient {
    pub fn same_key(&self, other: &Recipient) -> bool {
        self.key == other.key
    }

    pub fn is_ssh(&self) -> bool {
        matches!(self.kind, Kind::Ssh(_))
    }
}

impl fmt::Display for Recipient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.comment {
            Some(comment) => write!(f, "{} {comment}", self.key),
            None => f.write_str(&self.key),
        }
    }
}

pub fn recipient(input: &str) -> Result<Recipient, RecoveryError> {
    if input.len() > MAX_PUBLIC_KEY_BYTES {
        return Err(RecoveryError::NotAPublicKey);
    }
    if input.contains("AGE-SECRET-KEY-") || input.contains("PRIVATE KEY-----") {
        return Err(RecoveryError::PrivateKeyGiven);
    }

    let line = input.trim();
    if line.contains(['\n', '\r']) {
        return Err(RecoveryError::NotAPublicKey);
    }

    if let Ok(key) = age::x25519::Recipient::from_str(line) {
        return Ok(Recipient {
            key: key.to_string(),
            comment: None,
            kind: Kind::Age(key),
        });
    }

    let key = age::ssh::Recipient::from_str(line).map_err(|_| RecoveryError::NotAPublicKey)?;
    let comment: String = line
        .split_whitespace()
        .skip(2)
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_COMMENT_CHARS)
        .collect();
    Ok(Recipient {
        key: key.to_string(),
        comment: (!comment.is_empty()).then_some(comment),
        kind: Kind::Ssh(key),
    })
}

/// The master key sealed to `recipient`, ASCII armored.
pub fn seal(recipient: &Recipient, key: &MasterKey) -> Result<String, RecoveryError> {
    match &recipient.kind {
        Kind::Age(to) => age::encrypt_and_armor(to, key.as_ref()),
        Kind::Ssh(to) => age::encrypt_and_armor(to, key.as_ref()),
    }
    .map_err(|_| RecoveryError::Sealing)
}

/// Whether an SSH private key file is protected by a passphrase, or `None`
/// when it is not one.
pub fn ssh_key_has_passphrase(private_key: &[u8]) -> Option<bool> {
    let text = std::str::from_utf8(private_key).ok()?;
    if text.contains("-----BEGIN OPENSSH PRIVATE KEY-----") {
        return openssh_rounds(text).map(|rounds| rounds.is_some());
    }
    text.contains("PRIVATE KEY-----")
        .then(|| text.contains("ENCRYPTED"))
}

/// Whether an OpenSSH private key file holds the private half of `wanted`,
/// read from the public key the file carries in the clear.
pub fn ssh_key_matches(private_key: &[u8], wanted: &Recipient) -> bool {
    std::str::from_utf8(private_key)
        .ok()
        .and_then(openssh_public_key)
        .is_some_and(|public_key| public_key == wanted.key)
}

/// A value that tells master keys apart without revealing them.
pub fn check_value(key: &MasterKey) -> String {
    use blake2::digest::{consts::U32, KeyInit, Mac};
    let mut mac = <blake2::Blake2bMac<U32> as KeyInit>::new_from_slice(key.as_slice())
        .expect("a 32-byte key fits BLAKE2b");
    mac.update(b"wis-recovery-check");
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The master key in `sealed`, opened with an age identity file (passphrase
/// protected or not) or an SSH private key.
pub fn open(
    sealed: &str,
    private_key: &[u8],
    passphrase: Option<SecretString>,
) -> Result<MasterKey, RecoveryError> {
    if private_key.len() > MAX_KEY_FILE_BYTES {
        return Err(RecoveryError::NotAPrivateKey);
    }

    let unlocker = Unlocker::read(private_key)?;
    if sealed.len() > MAX_SEALED_BYTES {
        return Err(RecoveryError::Damaged);
    }

    let decryptor = age::Decryptor::new_buffered(age::armor::ArmoredReader::new(sealed.as_bytes()))
        .map_err(|_| RecoveryError::Damaged)?;
    let identities = unlocker.identities(passphrase)?;
    let mut reader = decryptor
        .decrypt(identities.iter().map(|identity| identity.as_ref()))
        .map_err(|err| match err {
            age::DecryptError::NoMatchingKeys => RecoveryError::WrongKey,
            _ => RecoveryError::Damaged,
        })?;
    let mut key = Zeroizing::new([0u8; 32]);
    reader
        .read_exact(key.as_mut())
        .map_err(|_| RecoveryError::Damaged)?;

    let mut extra = [0u8; 1];
    match reader.read(&mut extra) {
        Ok(0) => Ok(key),
        _ => Err(RecoveryError::Damaged),
    }
}

type Identities = Vec<Box<dyn age::Identity>>;

enum Unlocker<'a> {
    Ssh(age::ssh::Identity),
    Age(Identities),
    LockedAge(&'a [u8]),
}

impl<'a> Unlocker<'a> {
    fn read(private_key: &'a [u8]) -> Result<Self, RecoveryError> {
        let start = private_key.trim_ascii_start();
        if start.starts_with(b"-----BEGIN AGE ENCRYPTED FILE-----") {
            return Ok(Self::LockedAge(private_key));
        }

        if !start.starts_with(b"-----BEGIN") {
            let identities = age::IdentityFile::from_buffer(BufReader::new(private_key))
                .and_then(|file| file.into_identities().map_err(std::io::Error::other))
                .map_err(|_| RecoveryError::NotAPrivateKey)?;
            if identities.is_empty() {
                return Err(RecoveryError::NotAPrivateKey);
            }
            return Ok(Self::Age(
                identities
                    .into_iter()
                    .map(|identity| identity as Box<dyn age::Identity>)
                    .collect(),
            ));
        }

        let text = std::str::from_utf8(private_key).map_err(|_| RecoveryError::NotAPrivateKey)?;
        if text.contains("-----BEGIN OPENSSH PRIVATE KEY-----") {
            let rounds = openssh_rounds(text).ok_or(RecoveryError::NotAPrivateKey)?;
            if rounds.unwrap_or(0) > MAX_BCRYPT_ROUNDS {
                return Err(RecoveryError::ExcessiveWork);
            }
        }

        match age::ssh::Identity::from_buffer(Cursor::new(private_key), None) {
            Ok(age::ssh::Identity::Unsupported(_)) | Err(_) => Err(RecoveryError::NotAPrivateKey),
            Ok(identity) => Ok(Self::Ssh(identity)),
        }
    }

    fn identities(self, passphrase: Option<SecretString>) -> Result<Identities, RecoveryError> {
        match self {
            Self::Age(identities) => Ok(identities),
            Self::Ssh(age::ssh::Identity::Encrypted(locked)) => {
                let passphrase = passphrase.ok_or(RecoveryError::NeedsPassphrase)?;
                let unlocked = locked
                    .decrypt(passphrase)
                    .map_err(|_| RecoveryError::WrongPassphrase)?;
                Ok(vec![Box::new(age::ssh::Identity::from(unlocked))])
            }
            Self::Ssh(identity) => Ok(vec![Box::new(identity)]),
            Self::LockedAge(armored) => {
                let passphrase = passphrase.ok_or(RecoveryError::NeedsPassphrase)?;
                let decryptor = age::Decryptor::new(age::armor::ArmoredReader::new(armored))
                    .map_err(|_| RecoveryError::NotAPrivateKey)?;
                let locked = age::encrypted::EncryptedIdentity::new(
                    decryptor,
                    Passphrase(passphrase),
                    Some(MAX_SCRYPT_WORK_FACTOR),
                )
                .ok_or(RecoveryError::NotAPrivateKey)?;
                let file = locked.decrypt(None).map_err(|err| match err {
                    age::DecryptError::ExcessiveWork { .. } => RecoveryError::ExcessiveWork,
                    _ => RecoveryError::WrongPassphrase,
                })?;
                file.into_identities()
                    .map(|identities| {
                        identities
                            .into_iter()
                            .map(|identity| identity as Box<dyn age::Identity>)
                            .collect()
                    })
                    .map_err(|_| RecoveryError::NotAPrivateKey)
            }
        }
    }
}

#[derive(Clone)]
struct Passphrase(SecretString);

impl age::Callbacks for Passphrase {
    fn display_message(&self, _message: &str) {}

    fn confirm(&self, _message: &str, _yes: &str, _no: Option<&str>) -> Option<bool> {
        None
    }

    fn request_public_string(&self, _description: &str) -> Option<String> {
        None
    }

    fn request_passphrase(&self, _description: &str) -> Option<SecretString> {
        Some(self.0.clone())
    }
}

/// An OpenSSH private key's bcrypt rounds, `Some(None)` without a passphrase,
/// or `None` when the envelope cannot be read. Reads no further than age does
/// before deriving.
fn openssh_rounds(text: &str) -> Option<Option<u32>> {
    let raw = openssh_bytes(text)?;
    let mut rest = raw.strip_prefix(b"openssh-key-v1\0")?;
    openssh_kdf(&mut rest)
}

/// The public key an OpenSSH private key carries in the clear.
fn openssh_public_key(text: &str) -> Option<String> {
    let raw = openssh_bytes(text)?;
    let mut rest = raw.strip_prefix(b"openssh-key-v1\0")?;
    openssh_kdf(&mut rest)?;
    take_u32(&mut rest)?;
    let blob = take_string(&mut rest)?;
    let mut inner = blob;
    let kind = std::str::from_utf8(take_string(&mut inner)?).ok()?;
    Some(format!(
        "{kind} {}",
        base64::engine::general_purpose::STANDARD.encode(blob)
    ))
}

fn openssh_kdf(rest: &mut &[u8]) -> Option<Option<u32>> {
    let cipher = take_string(rest)?;
    let kdf = take_string(rest)?;
    let mut options = take_string(rest)?;
    match (cipher, kdf) {
        (b"none", b"none") => Some(None),
        (_, b"bcrypt") => {
            take_string(&mut options)?;
            take_u32(&mut options).map(Some)
        }
        _ => None,
    }
}

fn openssh_bytes(text: &str) -> Option<Zeroizing<Vec<u8>>> {
    let mut body = Zeroizing::new(String::with_capacity(text.len()));
    text.lines()
        .skip_while(|line| !line.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"))
        .skip(1)
        .take_while(|line| !line.starts_with("-----END"))
        .for_each(|line| body.push_str(line.trim()));
    base64::engine::general_purpose::STANDARD
        .decode(body.as_bytes())
        .ok()
        .map(Zeroizing::new)
}

fn take_u32(input: &mut &[u8]) -> Option<u32> {
    let (head, tail) = input.split_first_chunk::<4>()?;
    *input = tail;
    Some(u32::from_be_bytes(*head))
}

fn take_string<'a>(input: &mut &'a [u8]) -> Option<&'a [u8]> {
    let len = usize::try_from(take_u32(input)?).ok()?;
    if input.len() < len {
        return None;
    }
    let (head, tail) = input.split_at(len);
    *input = tail;
    Some(head)
}

#[cfg(test)]
mod tests {
    use age::secrecy::{ExposeSecret, SecretString};

    use super::*;
    use crate::generate_master_key;

    const SSH_PUBLIC: &str = include_str!("../testdata/recovery_ed25519.pub");
    const SSH_PRIVATE: &str = include_str!("../testdata/recovery_ed25519");
    const LOCKED_PUBLIC: &str = include_str!("../testdata/recovery_ed25519_locked.pub");
    const LOCKED_PRIVATE: &str = include_str!("../testdata/recovery_ed25519_locked");

    fn passphrase(text: &str) -> Option<SecretString> {
        Some(SecretString::from(text.to_string()))
    }

    #[test]
    fn a_key_sealed_to_an_age_key_opens_only_with_its_private_half() {
        let mine = age::x25519::Identity::generate();
        let theirs = age::x25519::Identity::generate();
        let key = generate_master_key();

        let sealed = seal(&recipient(&mine.to_public().to_string()).unwrap(), &key).unwrap();
        assert!(sealed.starts_with("-----BEGIN AGE ENCRYPTED FILE-----"));

        let opened = open(&sealed, mine.to_string().expose_secret().as_bytes(), None).unwrap();
        assert_eq!(*opened, *key);
        assert!(matches!(
            open(&sealed, theirs.to_string().expose_secret().as_bytes(), None),
            Err(RecoveryError::WrongKey)
        ));
    }

    #[test]
    fn an_ssh_key_works_and_a_locked_one_needs_its_passphrase() {
        let key = generate_master_key();

        let sealed = seal(&recipient(SSH_PUBLIC).unwrap(), &key).unwrap();
        assert_eq!(*open(&sealed, SSH_PRIVATE.as_bytes(), None).unwrap(), *key);

        let sealed = seal(&recipient(LOCKED_PUBLIC).unwrap(), &key).unwrap();
        assert!(matches!(
            open(&sealed, LOCKED_PRIVATE.as_bytes(), None),
            Err(RecoveryError::NeedsPassphrase)
        ));
        assert!(matches!(
            open(&sealed, LOCKED_PRIVATE.as_bytes(), passphrase("wrong")),
            Err(RecoveryError::WrongPassphrase)
        ));
        let opened = open(
            &sealed,
            LOCKED_PRIVATE.as_bytes(),
            passphrase("correct horse"),
        )
        .unwrap();
        assert_eq!(*opened, *key);

        assert!(matches!(
            open(&sealed, SSH_PRIVATE.as_bytes(), None),
            Err(RecoveryError::WrongKey)
        ));
    }

    #[test]
    fn only_a_public_key_is_taken_and_a_private_one_is_called_out() {
        let age_public = age::x25519::Identity::generate().to_public().to_string();
        assert_eq!(
            recipient(&format!("  {age_public}\n")).unwrap().to_string(),
            age_public
        );
        assert_eq!(
            recipient(SSH_PUBLIC).unwrap().to_string(),
            SSH_PUBLIC.trim()
        );

        for private in [
            SSH_PRIVATE.to_string(),
            age::x25519::Identity::generate()
                .to_string()
                .expose_secret()
                .to_string(),
        ] {
            assert!(matches!(
                recipient(&private),
                Err(RecoveryError::PrivateKeyGiven)
            ));
        }
        for junk in ["", "hello", "ssh-ed25519", "age1notakey"] {
            assert!(
                matches!(recipient(junk), Err(RecoveryError::NotAPublicKey)),
                "{junk}"
            );
        }
    }

    #[test]
    fn a_recipient_is_kept_as_its_key_and_a_short_printable_comment() {
        let long = format!("{} {}", SSH_PUBLIC.trim(), "x".repeat(300));
        let kept = recipient(&long).unwrap().to_string();
        assert!(kept.len() < SSH_PUBLIC.len() + 120, "{kept}");
        let plain = SSH_PUBLIC
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ");
        let odd = format!("{plain} bad\u{7}name");
        assert_eq!(
            recipient(&odd).unwrap().to_string(),
            format!("{plain} badname")
        );
        assert!(recipient(SSH_PUBLIC)
            .unwrap()
            .same_key(&recipient(&plain).unwrap()));

        let secret = age::x25519::Identity::generate()
            .to_string()
            .expose_secret()
            .to_string();
        let sneaky = format!("{}\n{secret}", SSH_PUBLIC.trim());
        assert!(matches!(
            recipient(&sneaky),
            Err(RecoveryError::PrivateKeyGiven)
        ));
        let two_lines = format!("{}\n{}", SSH_PUBLIC.trim(), SSH_PUBLIC.trim());
        assert!(matches!(
            recipient(&two_lines),
            Err(RecoveryError::NotAPublicKey)
        ));
        assert!(matches!(
            recipient(&"a".repeat(MAX_PUBLIC_KEY_BYTES + 1)),
            Err(RecoveryError::NotAPublicKey)
        ));
    }

    #[test]
    fn a_private_key_file_is_matched_to_its_public_key_from_the_file_alone() {
        let plain = recipient(SSH_PUBLIC).unwrap();
        let locked = recipient(LOCKED_PUBLIC).unwrap();
        assert!(ssh_key_matches(SSH_PRIVATE.as_bytes(), &plain));
        assert!(ssh_key_matches(LOCKED_PRIVATE.as_bytes(), &locked));
        assert!(!ssh_key_matches(LOCKED_PRIVATE.as_bytes(), &plain));
        assert!(!ssh_key_matches(b"not a key", &plain));
    }

    #[test]
    fn a_check_value_tells_keys_apart_without_revealing_them() {
        let key = generate_master_key();
        assert_eq!(check_value(&key), check_value(&key));
        assert_ne!(check_value(&key), check_value(&generate_master_key()));
        assert_eq!(check_value(&key).len(), 64);
        assert!(
            !check_value(&key).contains(&base64::engine::general_purpose::STANDARD.encode(*key))
        );
    }

    #[test]
    fn a_private_key_file_says_whether_it_has_a_passphrase() {
        assert_eq!(ssh_key_has_passphrase(SSH_PRIVATE.as_bytes()), Some(false));
        assert_eq!(
            ssh_key_has_passphrase(LOCKED_PRIVATE.as_bytes()),
            Some(true)
        );
        assert_eq!(ssh_key_has_passphrase(b"AGE-SECRET-KEY-1QQQ"), None);
    }

    fn tampered(private_key: &str, rounds: u32, break_public_key: bool) -> String {
        use base64::Engine;
        let body: String = private_key
            .lines()
            .filter(|line| !line.starts_with("-----"))
            .collect();
        let mut raw = base64::engine::general_purpose::STANDARD
            .decode(body)
            .unwrap();
        let options = raw.windows(6).position(|w| w == b"bcrypt").unwrap() + 6;
        let options_len =
            u32::from_be_bytes(raw[options..options + 4].try_into().unwrap()) as usize;
        let rounds_at = options + 4 + 4 + 16;
        raw[rounds_at..rounds_at + 4].copy_from_slice(&rounds.to_be_bytes());
        if break_public_key {
            let blob = options + 4 + options_len + 4 + 4;
            raw[blob..blob + 4].copy_from_slice(&u32::MAX.to_be_bytes());
        }
        let encoded = base64::engine::general_purpose::STANDARD.encode(raw);
        format!(
            "-----BEGIN OPENSSH PRIVATE KEY-----\n{encoded}\n-----END OPENSSH PRIVATE KEY-----\n"
        )
    }

    #[test]
    fn a_key_demanding_too_much_work_is_refused_before_trying() {
        let key = generate_master_key();
        let sealed = seal(&recipient(LOCKED_PUBLIC).unwrap(), &key).unwrap();

        for costly in [
            tampered(LOCKED_PRIVATE, u32::MAX, false),
            tampered(LOCKED_PRIVATE, u32::MAX, true),
        ] {
            let started = std::time::Instant::now();
            assert!(matches!(
                open(&sealed, costly.as_bytes(), passphrase("correct horse")),
                Err(RecoveryError::ExcessiveWork)
            ));
            assert!(started.elapsed() < std::time::Duration::from_secs(1));
        }
    }

    #[test]
    fn a_passphrase_protected_age_identity_opens_with_its_passphrase() {
        let mine = age::x25519::Identity::generate();
        let key = generate_master_key();
        let sealed = seal(&recipient(&mine.to_public().to_string()).unwrap(), &key).unwrap();
        let mut lock = age::scrypt::Recipient::new(SecretString::from("tiger".to_string()));
        lock.set_work_factor(10);
        let protected =
            age::encrypt_and_armor(&lock, mine.to_string().expose_secret().as_bytes()).unwrap();

        assert!(matches!(
            open(&sealed, protected.as_bytes(), None),
            Err(RecoveryError::NeedsPassphrase)
        ));
        assert!(matches!(
            open(&sealed, protected.as_bytes(), passphrase("lion")),
            Err(RecoveryError::WrongPassphrase)
        ));
        let opened = open(&sealed, protected.as_bytes(), passphrase("tiger")).unwrap();
        assert_eq!(*opened, *key);
    }

    #[test]
    fn oversized_or_damaged_input_is_refused() {
        let mine = age::x25519::Identity::generate();
        let identity = mine.to_string().expose_secret().to_string();
        let huge = "a".repeat(MAX_KEY_FILE_BYTES + 1);
        assert!(matches!(
            open("", huge.as_bytes(), None),
            Err(RecoveryError::NotAPrivateKey)
        ));
        assert!(matches!(
            open("not age", identity.as_bytes(), None),
            Err(RecoveryError::Damaged)
        ));
        assert!(matches!(
            open("", b"hello", None),
            Err(RecoveryError::NotAPrivateKey)
        ));

        let short = age::encrypt_and_armor(&mine.to_public(), b"short").unwrap();
        assert!(matches!(
            open(&short, identity.as_bytes(), None),
            Err(RecoveryError::Damaged)
        ));
        let long = age::encrypt_and_armor(&mine.to_public(), &[7u8; 33]).unwrap();
        assert!(matches!(
            open(&long, identity.as_bytes(), None),
            Err(RecoveryError::Damaged)
        ));
    }
}
