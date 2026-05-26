//! Local certificate authority used by the MITM proxy.
//!
//! At first launch we generate a self-signed CA, persist it under the
//! Tauri-resolved app-data directory, and reuse it on subsequent runs. The
//! user must trust this CA in their OS keychain for the MITM tunnel to
//! present per-host leaf certificates that the WebView accepts.
//!
//! The functional core lives here: pure functions that produce
//! [`rustls::ServerConfig`] values from an SNI hostname. The shell layer
//! handles I/O (loading/persisting PEM files, settings toggles).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair, KeyUsagePurpose,
    SanType,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::ServerConfig;
use time::{Duration as TimeDuration, OffsetDateTime};
use tokio::sync::Mutex;

use crate::error::{NetworkError, NetworkResult};

const CA_CERT_FILE: &str = "wis-ca.pem";
const CA_KEY_FILE: &str = "wis-ca.key";

/// 10 years. Long enough that users don't need to re-trust frequently;
/// short enough that a long-lived keychain entry will eventually expire.
const CA_VALIDITY_DAYS: i64 = 365 * 10;

/// 30 days. Leaves are issued on demand and cached for minutes; if the cache
/// is evicted a fresh one is signed. The 30-day window only matters when a
/// leaf outlives the proxy process, where it shouldn't.
const LEAF_VALIDITY_DAYS: i64 = 30;

/// Cache TTL for the per-SNI `ServerConfig`. Short enough that a revoked
/// upstream cert (normally caught on the outbound TLS handshake)
/// stops being trusted within a few minutes.
const CACHE_TTL: Duration = Duration::from_secs(5 * 60);

/// The persistent Wis local CA.
pub struct CertAuthority {
    cert_pem: String,
    cert_der: CertificateDer<'static>,
    key_pair: KeyPair,
    /// Rebuilt from `(cert_pem, key_pair)` at load/generate time and used to
    /// sign leaves. Its DER is not the same as `cert_der` because rcgen
    /// re-stamps the validity dates each load, but its subject DN matches,
    /// so leaves it signs chain to the on-disk CA the user trusted.
    signing_cert: rcgen::Certificate,
}

impl CertAuthority {
    /// Load the CA from `dir`, generating one if missing. `dir` is created
    /// if absent.
    pub fn load_or_generate(dir: &Path) -> NetworkResult<Self> {
        std::fs::create_dir_all(dir)?;
        let cert_path = dir.join(CA_CERT_FILE);
        let key_path = dir.join(CA_KEY_FILE);

        if cert_path.exists() && key_path.exists() {
            Self::load(&cert_path, &key_path)
        } else {
            Self::generate(&cert_path, &key_path)
        }
    }

    fn generate(cert_path: &Path, key_path: &Path) -> NetworkResult<Self> {
        let key_pair =
            KeyPair::generate().map_err(|e| NetworkError::Config(format!("CA key: {e}")))?;
        let cert = ca_params()?
            .self_signed(&key_pair)
            .map_err(|e| NetworkError::Config(format!("CA self-sign: {e}")))?;

        let cert_pem = cert.pem();
        let key_pem = key_pair.serialize_pem();

        std::fs::write(cert_path, &cert_pem)?;
        write_with_owner_only(key_path, &key_pem)?;

        Ok(Self {
            cert_pem,
            cert_der: cert.der().clone(),
            key_pair,
            signing_cert: cert,
        })
    }

    /// Rebuild a signing-capable `Certificate` from the persisted private
    /// key plus the same DN parameters used at generation. The resulting
    /// object has a different fingerprint to the on-disk PEM (validity
    /// dates differ) but signs leaves whose Issuer DN matches it. The
    /// keychain validates those leaves against the persisted CA cert by
    /// matching DNs and verifying the signature with the shared public key.
    fn load(cert_path: &Path, key_path: &Path) -> NetworkResult<Self> {
        let cert_pem = std::fs::read_to_string(cert_path)?;
        let key_pem = std::fs::read_to_string(key_path)?;

        let key_pair = KeyPair::from_pem(&key_pem)
            .map_err(|e| NetworkError::Config(format!("CA key parse: {e}")))?;
        let signing_cert = ca_params()?
            .self_signed(&key_pair)
            .map_err(|e| NetworkError::Config(format!("CA reload: {e}")))?;

        let cert_der = parse_cert_pem(&cert_pem)?;

        Ok(Self {
            cert_pem,
            cert_der,
            key_pair,
            signing_cert,
        })
    }

    /// The CA certificate in PEM form. This is what the user installs into
    /// their OS keychain.
    pub fn cert_pem(&self) -> &str {
        &self.cert_pem
    }

    fn issue_leaf(
        &self,
        sni: &str,
    ) -> NetworkResult<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
        let mut params = CertificateParams::new(vec![sni.to_string()])
            .map_err(|e| NetworkError::Config(format!("leaf params: {e}")))?;
        params.distinguished_name = {
            let mut dn = DistinguishedName::new();
            dn.push(DnType::CommonName, sni);
            dn
        };
        params.subject_alt_names =
            vec![SanType::DnsName(sni.to_string().try_into().map_err(
                |e: rcgen::Error| NetworkError::Config(format!("SAN: {e}")),
            )?)];
        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyEncipherment,
        ];

        let now = OffsetDateTime::now_utc();
        params.not_before = now - TimeDuration::hours(1);
        params.not_after = now + TimeDuration::days(LEAF_VALIDITY_DAYS);

        let leaf_key =
            KeyPair::generate().map_err(|e| NetworkError::Config(format!("leaf key: {e}")))?;
        let leaf_cert = params
            .signed_by(&leaf_key, &self.signing_cert, &self.key_pair)
            .map_err(|e| NetworkError::Config(format!("leaf sign: {e}")))?;

        let chain = vec![leaf_cert.der().clone(), self.cert_der.clone()];
        let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der()));

        Ok((chain, key_der))
    }
}

/// Per-host server config cache.
#[derive(Clone)]
pub struct CertIssuer {
    ca: Arc<CertAuthority>,
    cache: Arc<Mutex<HashMap<String, CacheEntry>>>,
}

struct CacheEntry {
    config: Arc<ServerConfig>,
    issued_at: Instant,
}

impl CertIssuer {
    pub fn new(ca: Arc<CertAuthority>) -> Self {
        Self {
            ca,
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get (or mint) a `ServerConfig` valid for the given SNI hostname.
    pub async fn server_config_for(&self, sni: &str) -> NetworkResult<Arc<ServerConfig>> {
        {
            let mut cache = self.cache.lock().await;
            if let Some(entry) = cache.get(sni) {
                if entry.issued_at.elapsed() < CACHE_TTL {
                    return Ok(entry.config.clone());
                }
                cache.remove(sni);
            }
        }

        let (chain, key) = self.ca.issue_leaf(sni)?;
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(chain, key)
            .map_err(|e| NetworkError::Config(format!("ServerConfig: {e}")))?;
        let config = Arc::new(config);

        let mut cache = self.cache.lock().await;
        cache.insert(
            sni.to_string(),
            CacheEntry {
                config: config.clone(),
                issued_at: Instant::now(),
            },
        );

        Ok(config)
    }

    /// Borrow the underlying CA (for `ca_pem()` export).
    pub fn ca(&self) -> &CertAuthority {
        &self.ca
    }
}

/// Bundle of everything the MITM tunnel needs: how to mint server configs
/// for the WebView side, and the upstream client config used to talk to the
/// real origin.
#[derive(Clone)]
pub struct MitmConfig {
    pub issuer: CertIssuer,
    pub client_config: Arc<rustls::ClientConfig>,
}

impl MitmConfig {
    pub fn new(ca: Arc<CertAuthority>) -> NetworkResult<Self> {
        let issuer = CertIssuer::new(ca);

        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let client_config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();

        Ok(Self {
            issuer,
            client_config: Arc::new(client_config),
        })
    }
}

/// CA certificate parameters: hard-coded DN, key usage, and validity so
/// both `generate` and `load` produce a certificate with the same Subject
/// DN. Leaves signed by either chain to the on-disk CA.
fn ca_params() -> NetworkResult<CertificateParams> {
    let mut params = CertificateParams::new(Vec::<String>::new())
        .map_err(|e| NetworkError::Config(format!("CA params: {e}")))?;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::CrlSign,
    ];
    
    let now = OffsetDateTime::now_utc();
    params.not_before = now - TimeDuration::hours(1);
    params.not_after = now + TimeDuration::days(CA_VALIDITY_DAYS);
    params.distinguished_name = {
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, "Wis Local CA");
        dn.push(DnType::OrganizationName, "Wis");
        dn
    };

    Ok(params)
}

fn parse_cert_pem(pem: &str) -> NetworkResult<CertificateDer<'static>> {
    let mut cursor = std::io::Cursor::new(pem.as_bytes());
    let mut iter = rustls_pemfile::certs(&mut cursor);

    iter.next()
        .ok_or_else(|| NetworkError::Config("CA PEM contains no certificate".into()))?
        .map_err(|e| NetworkError::Config(format!("CA PEM read: {e}")))
}

fn write_with_owner_only(path: &Path, content: &str) -> NetworkResult<()> {
    std::fs::write(path, content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(path, perms)?;
    }
    let _ = path;

    Ok(())
}

/// Convenience for the shell: derive the canonical CA directory inside
/// `app_data_dir`. Returns `<app_data_dir>/ca`.
pub fn ca_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("ca")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_or_generate_is_idempotent() {
        let tmp = tempdir();
        let a = CertAuthority::load_or_generate(&tmp).expect("first");
        let b = CertAuthority::load_or_generate(&tmp).expect("second");
        
        // Same CA PEM both times (persist + reload, never regenerate when
        // the files already exist).
        assert_eq!(a.cert_pem(), b.cert_pem());
        // The on-disk private key permissions are owner-only on unix.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(tmp.join("wis-ca.key"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[tokio::test]
    async fn cert_issuer_caches_per_sni_within_ttl() {
        let tmp = tempdir();
        let ca = Arc::new(CertAuthority::load_or_generate(&tmp).expect("ca"));
        let issuer = CertIssuer::new(ca);
        let a = issuer.server_config_for("example.com").await.expect("a");
        let b = issuer.server_config_for("example.com").await.expect("b");
        assert!(Arc::ptr_eq(&a, &b), "cache hit should return the same Arc");

        let c = issuer.server_config_for("other.example").await.expect("c");
        assert!(
            !Arc::ptr_eq(&a, &c),
            "different SNIs must produce different configs"
        );
    }

    fn tempdir() -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "wis-ca-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
