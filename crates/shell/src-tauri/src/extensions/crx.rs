//! Chrome Web Store packages (CRX3): the signed header Chromium checks before
//! it trusts the ZIP that follows, as in its `crx_verifier.cc`.

use ring::digest::{digest, SHA256};
use ring::signature::{
    UnparsedPublicKey, ECDSA_P256_SHA256_ASN1, RSA_PKCS1_1024_8192_SHA256_FOR_LEGACY_USE_ONLY,
};
use spki::{ObjectIdentifier, SubjectPublicKeyInfoRef};

/// SHA-256 of the DER public key Google signs Web Store packages with.
pub(super) const STORE_PUBLISHER: [u8; 32] = [
    0x61, 0xf7, 0xf2, 0xa6, 0xbf, 0xcf, 0x74, 0xcd, 0x0b, 0xc1, 0xfe, 0x24, 0x97, 0xcc, 0x9b, 0x04,
    0x25, 0x4c, 0x65, 0x8f, 0x79, 0xf2, 0x14, 0x53, 0x92, 0x86, 0x7e, 0xa8, 0x36, 0x63, 0x67, 0xcf,
];

const MAGIC: &[u8] = b"Cr24";
const VERSION: u32 = 3;
const CONTEXT: &[u8] = b"CRX3 SignedData\x00";
const MAX_HEADER: usize = 1024 * 1024;
/// Each proof is checked over the whole archive; real packages carry three.
const MAX_PROOFS: usize = 16;
const RSA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1");
const EC: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.2.1");

#[derive(Debug, PartialEq, Eq)]
pub(super) enum CrxError {
    NotCrx3,
    BadHeader,
    BadSignature,
    WrongExtension,
    NotFromStore,
}

impl std::fmt::Display for CrxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            CrxError::NotCrx3 => "the download is not an extension package",
            CrxError::BadHeader => "the extension package is damaged",
            CrxError::BadSignature => "the extension package's signature does not match it",
            CrxError::WrongExtension => "the store sent a different extension",
            CrxError::NotFromStore => "the extension package is not signed by the Chrome Web Store",
        })
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Rsa,
    Ecdsa,
}

fn varint(bytes: &mut &[u8]) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let (&byte, rest) = bytes.split_first()?;
        *bytes = rest;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

/// A protobuf message's length-delimited fields, by number, skipping the rest.
fn fields(mut bytes: &[u8]) -> Option<Vec<(u64, &[u8])>> {
    let mut found = Vec::new();
    while !bytes.is_empty() {
        let key = varint(&mut bytes)?;
        let skip = match key & 7 {
            0 => {
                varint(&mut bytes)?;
                0
            }
            1 => 8,
            2 => {
                let len = usize::try_from(varint(&mut bytes)?).ok()?;
                let value = bytes.get(..len)?;
                found.push((key >> 3, value));
                len
            }
            5 => 4,
            _ => return None,
        };
        bytes = bytes.get(skip..)?;
    }

    Some(found)
}

/// A field's last value, as protobuf reads repeated singular fields.
fn last<'a>(fields: &[(u64, &'a [u8])], number: u64) -> &'a [u8] {
    fields
        .iter()
        .rev()
        .find(|(n, _)| *n == number)
        .map_or(&[], |(_, value)| *value)
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    bytes
        .get(at..at + 4)?
        .try_into()
        .ok()
        .map(u32::from_le_bytes)
}

/// Chromium's extension id for a public key: the first 16 bytes of its
/// SHA-256, each hex digit written as a letter from `a` to `p`.
pub(super) fn id_of(key: &[u8]) -> String {
    id_from_bytes(&digest(&SHA256, key).as_ref()[..16])
}

fn id_from_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .flat_map(|b| [b >> 4, b & 0xf])
        .map(|nibble| char::from(b'a' + nibble))
        .collect()
}

fn verifies(kind: Kind, key: &[u8], signature: &[u8], message: &[u8]) -> bool {
    let Ok(info) = SubjectPublicKeyInfoRef::try_from(key) else {
        return false;
    };
    let Some(point) = info.subject_public_key.as_bytes() else {
        return false;
    };
    match kind {
        Kind::Rsa if info.algorithm.oid == RSA => {
            UnparsedPublicKey::new(&RSA_PKCS1_1024_8192_SHA256_FOR_LEGACY_USE_ONLY, point)
                .verify(message, signature)
                .is_ok()
        }
        Kind::Ecdsa if info.algorithm.oid == EC => {
            UnparsedPublicKey::new(&ECDSA_P256_SHA256_ASN1, point)
                .verify(message, signature)
                .is_ok()
        }
        _ => false,
    }
}

/// Where the ZIP inside `crx` starts, once every proof in it holds, one of
/// them is the developer's key for `id`, and another is the key hashed to
/// `publisher`.
pub(super) fn verified_archive(
    crx: &[u8],
    id: &str,
    publisher: &[u8; 32],
) -> Result<usize, CrxError> {
    if crx.get(..4) != Some(MAGIC) || u32_at(crx, 4) != Some(VERSION) {
        return Err(CrxError::NotCrx3);
    }

    let size = u32_at(crx, 8)
        .and_then(|s| usize::try_from(s).ok())
        .filter(|s| *s <= MAX_HEADER)
        .ok_or(CrxError::BadHeader)?;
    let header = crx.get(12..12 + size).ok_or(CrxError::BadHeader)?;
    let archive = &crx[12 + size..];
    let markers: [&[u8]; 3] = [b"PK\x05\x06", b"PK\x06\x07", b"PK\x06\x06"];
    if markers.iter().any(|m| header.windows(4).any(|w| w == *m)) {
        return Err(CrxError::BadHeader);
    }

    let header = fields(header).ok_or(CrxError::BadHeader)?;
    let signed = last(&header, 10000);
    let declared = id_from_bytes(last(&fields(signed).ok_or(CrxError::BadHeader)?, 1));
    let proofs = header
        .iter()
        .filter_map(|(number, proof)| match number {
            2 => Some((Kind::Rsa, *proof)),
            3 => Some((Kind::Ecdsa, *proof)),
            _ => None,
        })
        .map(|(kind, proof)| {
            let proof = fields(proof)?;
            Some((kind, last(&proof, 1), last(&proof, 2)))
        })
        .collect::<Option<Vec<_>>>()
        .filter(|proofs| proofs.len() <= MAX_PROOFS)
        .ok_or(CrxError::BadHeader)?;

    if declared != id || !proofs.iter().any(|(_, key, _)| id_of(key) == declared) {
        return Err(CrxError::WrongExtension);
    }
    if !proofs
        .iter()
        .any(|(_, key, _)| digest(&SHA256, key).as_ref() == publisher)
    {
        return Err(CrxError::NotFromStore);
    }

    let signed_size = u32::try_from(signed.len()).map_err(|_| CrxError::BadHeader)?;
    let message = [CONTEXT, &signed_size.to_le_bytes(), signed, archive].concat();
    if !proofs
        .into_iter()
        .all(|(kind, key, signature)| verifies(kind, key, signature, &message))
    {
        return Err(CrxError::BadSignature);
    }

    Ok(12 + size)
}

#[cfg(test)]
mod tests {
    use ring::rand::SystemRandom;
    use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_ASN1_SIGNING};

    use super::*;

    const P256_SPKI_PREFIX: [u8; 26] = [
        0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08,
        0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
    ];

    struct Signer {
        pair: EcdsaKeyPair,
        spki: Vec<u8>,
    }

    fn signer() -> Signer {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng).unwrap();
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng)
            .unwrap();
        let spki = [P256_SPKI_PREFIX.as_slice(), pair.public_key().as_ref()].concat();
        Signer { pair, spki }
    }

    fn hash(key: &[u8]) -> [u8; 32] {
        digest(&SHA256, key).as_ref().try_into().unwrap()
    }

    fn put_varint(out: &mut Vec<u8>, mut value: u64) {
        while value >= 0x80 {
            out.push((value as u8) | 0x80);
            value >>= 7;
        }
        out.push(value as u8);
    }

    fn field(number: u64, value: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        put_varint(&mut out, (number << 3) | 2);
        put_varint(&mut out, value.len() as u64);
        out.extend_from_slice(value);
        out
    }

    /// A package of `archive` signed by each of `signers`, naming `crx_id`.
    fn package(signers: &[&Signer], crx_id: &[u8], archive: &[u8]) -> Vec<u8> {
        let signed = field(1, crx_id);
        let message = [
            CONTEXT,
            &(signed.len() as u32).to_le_bytes(),
            &signed,
            archive,
        ]
        .concat();
        let rng = SystemRandom::new();
        let mut header = Vec::new();
        for signer in signers {
            let signature = signer.pair.sign(&rng, &message).unwrap();
            let proof = [field(1, &signer.spki), field(2, signature.as_ref())].concat();
            header.extend(field(3, &proof));
        }
        header.extend(field(10000, &signed));
        [
            MAGIC,
            &VERSION.to_le_bytes(),
            &(header.len() as u32).to_le_bytes(),
            &header,
            archive,
        ]
        .concat()
    }

    fn crx_id(signer: &Signer) -> Vec<u8> {
        digest(&SHA256, &signer.spki).as_ref()[..16].to_vec()
    }

    const ARCHIVE: &[u8] = b"PK\x03\x04 pretend zip";

    #[test]
    fn ids_are_hex_digits_as_letters() {
        assert_eq!(id_of(b"test"), "jpignaibiiemhngfjkcpokkamffknabf");
    }

    #[test]
    fn a_store_package_gives_its_archive() {
        let (developer, store) = (signer(), signer());
        let crx = package(&[&developer, &store], &crx_id(&developer), ARCHIVE);
        let id = id_of(&developer.spki);
        let start = verified_archive(&crx, &id, &hash(&store.spki)).unwrap();
        assert_eq!(&crx[start..], ARCHIVE);
    }

    #[test]
    fn a_package_the_store_did_not_sign_is_refused() {
        let (developer, store) = (signer(), signer());
        let crx = package(&[&developer], &crx_id(&developer), ARCHIVE);
        let id = id_of(&developer.spki);
        assert_eq!(
            verified_archive(&crx, &id, &hash(&store.spki)),
            Err(CrxError::NotFromStore)
        );
    }

    #[test]
    fn a_package_for_another_extension_is_refused() {
        let (developer, store, other) = (signer(), signer(), signer());
        let crx = package(&[&developer, &store], &crx_id(&developer), ARCHIVE);
        assert_eq!(
            verified_archive(&crx, &id_of(&other.spki), &hash(&store.spki)),
            Err(CrxError::WrongExtension)
        );
        let claims_other = package(&[&developer, &store], &crx_id(&other), ARCHIVE);
        assert_eq!(
            verified_archive(&claims_other, &id_of(&other.spki), &hash(&store.spki)),
            Err(CrxError::WrongExtension)
        );
    }

    #[test]
    fn a_changed_archive_is_refused() {
        let (developer, store) = (signer(), signer());
        let mut crx = package(&[&developer, &store], &crx_id(&developer), ARCHIVE);
        *crx.last_mut().unwrap() ^= 1;
        let id = id_of(&developer.spki);
        assert_eq!(
            verified_archive(&crx, &id, &hash(&store.spki)),
            Err(CrxError::BadSignature)
        );
    }

    #[test]
    fn other_formats_and_damage_are_refused() {
        let (developer, store) = (signer(), signer());
        let crx = package(&[&developer, &store], &crx_id(&developer), ARCHIVE);
        let id = id_of(&developer.spki);
        let publisher = hash(&store.spki);

        let mut crx2 = crx.clone();
        crx2[4] = 2;
        assert_eq!(
            verified_archive(&crx2, &id, &publisher),
            Err(CrxError::NotCrx3)
        );
        assert_eq!(
            verified_archive(b"PK\x03\x04", &id, &publisher),
            Err(CrxError::NotCrx3)
        );
        assert_eq!(
            verified_archive(&crx[..20], &id, &publisher),
            Err(CrxError::BadHeader)
        );

        let mut huge = crx.clone();
        huge[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            verified_archive(&huge, &id, &publisher),
            Err(CrxError::BadHeader)
        );
    }

    #[test]
    fn a_header_with_too_many_proofs_is_refused() {
        let (developer, store) = (signer(), signer());
        let many: Vec<&Signer> = std::iter::once(&store)
            .cycle()
            .take(MAX_PROOFS)
            .chain(std::iter::once(&developer))
            .collect();
        let crx = package(&many, &crx_id(&developer), ARCHIVE);
        let id = id_of(&developer.spki);
        assert_eq!(
            verified_archive(&crx, &id, &hash(&store.spki)),
            Err(CrxError::BadHeader)
        );
    }

    #[test]
    fn a_header_with_a_zip_directory_marker_is_refused() {
        let (developer, store) = (signer(), signer());
        let mut crx = package(&[&developer, &store], &crx_id(&developer), ARCHIVE);
        let header_end = 12 + u32::from_le_bytes(crx[8..12].try_into().unwrap()) as usize;
        let marker = [b"PK\x05\x06".as_slice(), &[0; 4]].concat();
        let padding = field(7, &marker);
        let header = [crx[12..header_end].to_vec(), padding].concat();
        crx = [
            &crx[..8],
            (header.len() as u32).to_le_bytes().as_slice(),
            &header,
            &crx[header_end..],
        ]
        .concat();
        let id = id_of(&developer.spki);
        assert_eq!(
            verified_archive(&crx, &id, &hash(&store.spki)),
            Err(CrxError::BadHeader)
        );
    }

    /// `WIS_CRX=path/to/package.crx WIS_CRX_ID=<store id> cargo test -- --ignored`
    #[test]
    #[ignore]
    fn a_real_store_package_verifies() {
        let path = std::env::var("WIS_CRX").unwrap();
        let id = std::env::var("WIS_CRX_ID").unwrap();
        let crx = std::fs::read(path).unwrap();
        let start = verified_archive(&crx, &id, &STORE_PUBLISHER).unwrap();
        assert!(crx[start..].starts_with(b"PK\x03\x04"));
    }
}
