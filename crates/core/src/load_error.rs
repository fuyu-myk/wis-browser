//! A failed page load, normalized from each engine's native error into a
//! small, UI-facing shape. The per-platform code mapping is pure
//! (mirroring [`crate::link_safety`]).

use serde::{Deserialize, Serialize};

/// The class of a load failure, chosen for the message the error page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadErrorKind {
    /// The host name could not be resolved.
    DnsFailure,
    /// The connection was refused or the server is unreachable.
    ConnectionRefused,
    /// The connection or request timed out.
    Timeout,
    /// The TLS certificate could not be validated.
    TlsError,
    /// The device appears to be offline.
    Offline,
    /// A failure that does not fit the buckets above.
    Unknown,
}

/// Why a TLS certificate was rejected, chosen for the interstitial's message
/// and for deciding whether a user bypass is offered at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TlsFault {
    /// The certificate is expired or not yet valid.
    Expired,
    /// The certificate was issued for a different host name.
    NameMismatch,
    /// The chain is self-signed, has an unknown root, or is otherwise
    /// untrusted.
    Untrusted,
    /// The certificate was revoked by its issuer. Never bypassable.
    Revoked,
    /// The secure channel itself failed (protocol or client-certificate
    /// fault); accepting the server's certificate would not help. Never
    /// bypassable.
    Protocol,
}

impl TlsFault {
    /// Map Linux `GTlsCertificateFlags` bits to the most severe fault set.
    ///
    /// Bit values verified against gio-sys (`G_TLS_CERTIFICATE_*`):
    /// 1 unknown-CA, 2 bad-identity, 4 not-activated, 8 expired, 16 revoked,
    /// 32 insecure, 64 generic-error.
    ///
    /// Empty or unrecognized flags fail closed (`Protocol`, no bypass).
    fn from_gtls_flags(flags: u32) -> Self {
        if flags & 16 != 0 {
            Self::Revoked
        } else if flags & (4 | 8) != 0 {
            Self::Expired
        } else if flags & 2 != 0 {
            Self::NameMismatch
        } else if flags & (1 | 32) != 0 {
            Self::Untrusted
        } else {
            Self::Protocol
        }
    }
}

/// A transient failed page load surfaced to the chrome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadError {
    pub kind: LoadErrorKind,
    /// The engine's native error code, kept for diagnostics only.
    pub code: i64,
    /// The URL whose load failed.
    pub url: String,
    /// The certificate fault behind a `TlsError`, `None` otherwise.
    #[serde(default)]
    pub tls: Option<TlsFault>,
}

impl LoadError {
    /// Build from a macOS `NSError` domain + code (WKWebView navigation
    /// failures). Returns `None` for user-cancelled loads, which must not
    /// surface an error page.
    pub fn from_ns_error(domain: &str, code: i64, url: String) -> Option<Self> {
        if domain == "NSURLErrorDomain" {
            // NSURLErrorCancelled: user or app stopped the load.
            if code == -999 {
                return None;
            }

            let (kind, tls) = match code {
                -1003 | -1006 => (LoadErrorKind::DnsFailure, None), // host not found / DNS lookup failed
                -1004 => (LoadErrorKind::ConnectionRefused, None),  // cannot connect to host
                -1001 => (LoadErrorKind::Timeout, None),
                -1009 => (LoadErrorKind::Offline, None), // not connected to internet
                // TLS/certificate failures (-1200 secure connection failed, plus
                // the -1206..=-1201 certificate-specific band).
                -1201 | -1204 => (LoadErrorKind::TlsError, Some(TlsFault::Expired)), // bad date / not yet valid
                -1202 | -1203 => (LoadErrorKind::TlsError, Some(TlsFault::Untrusted)), // untrusted / unknown root
                -1206..=-1200 => (LoadErrorKind::TlsError, Some(TlsFault::Protocol)), // handshake / client cert
                _ => (LoadErrorKind::Unknown, None),
            };

            return Some(Self {
                kind,
                code,
                url,
                tls,
            });
        }

        // Non-URL-loading domains (e.g. WebKitErrorDomain plug-in failures) are
        // not network faults worth an error page.
        None
    }

    /// Build from a Windows `COREWEBVIEW2_WEB_ERROR_STATUS` value. Returns
    /// `None` for the user-cancelled status.
    pub fn from_web_error_status(status: u32, url: String) -> Option<Self> {
        // Discriminants of COREWEBVIEW2_WEB_ERROR_STATUS (webview2-com-sys):
        // 1 cert-name-incorrect, 2 cert-expired, 3 client-cert-errors,
        // 4 cert-revoked, 5 cert-invalid | 6 server-unreachable, 7 timeout |
        // 9 conn-aborted, 10 conn-reset, 12 cannot-connect | 11 disconnected |
        // 13 host-not-resolved | 14 operation-canceled.
        let (kind, tls) = match status {
            1 => (LoadErrorKind::TlsError, Some(TlsFault::NameMismatch)),
            2 => (LoadErrorKind::TlsError, Some(TlsFault::Expired)),
            3 => (LoadErrorKind::TlsError, Some(TlsFault::Protocol)),
            4 => (LoadErrorKind::TlsError, Some(TlsFault::Revoked)),
            5 => (LoadErrorKind::TlsError, Some(TlsFault::Untrusted)),
            6 | 9 | 10 | 12 => (LoadErrorKind::ConnectionRefused, None),
            7 => (LoadErrorKind::Timeout, None),
            11 => (LoadErrorKind::Offline, None),
            13 => (LoadErrorKind::DnsFailure, None),
            14 => return None,
            _ => (LoadErrorKind::Unknown, None),
        };

        Some(Self {
            kind,
            code: status as i64,
            url,
            tls,
        })
    }

    /// Build from a Linux WebKitGTK load failure. `tls_flags` carries the
    /// `GTlsCertificateFlags` bits of the `load-failed-with-tls-errors`
    /// signal (`None` for the plain `load-failed` signal); `cancelled` marks
    /// a user/app stop. WebKitGTK's `NetworkError` is coarse (no dependable
    /// DNS/timeout discriminant), so a non-TLS failure surfaces as `Unknown`.
    /// Returns `None` when cancelled.
    pub fn from_glib(cancelled: bool, tls_flags: Option<u32>, url: String) -> Option<Self> {
        if cancelled {
            return None;
        }

        Some(match tls_flags {
            Some(flags) => Self {
                kind: LoadErrorKind::TlsError,
                code: i64::from(flags),
                url,
                tls: Some(TlsFault::from_gtls_flags(flags)),
            },
            None => Self {
                kind: LoadErrorKind::Unknown,
                code: 0,
                url,
                tls: None,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ns_error_maps_common_codes() {
        let cases = [
            (-1003, LoadErrorKind::DnsFailure),
            (-1004, LoadErrorKind::ConnectionRefused),
            (-1001, LoadErrorKind::Timeout),
            (-1009, LoadErrorKind::Offline),
            (-1202, LoadErrorKind::TlsError),
            (-4242, LoadErrorKind::Unknown),
        ];
        for (code, kind) in cases {
            let err = LoadError::from_ns_error("NSURLErrorDomain", code, "https://x/".into())
                .expect("should surface");
            assert_eq!(err.kind, kind, "code {code}");
            assert_eq!(err.code, code);
        }
    }

    #[test]
    fn ns_error_cancelled_and_foreign_domains_are_none() {
        assert!(LoadError::from_ns_error("NSURLErrorDomain", -999, "https://x/".into()).is_none());
        assert!(LoadError::from_ns_error("WebKitErrorDomain", 204, "https://x/".into()).is_none());
    }

    #[test]
    fn web_error_status_maps_and_skips_cancel() {
        let cases = [
            (2, LoadErrorKind::TlsError),
            (6, LoadErrorKind::ConnectionRefused),
            (7, LoadErrorKind::Timeout),
            (11, LoadErrorKind::Offline),
            (13, LoadErrorKind::DnsFailure),
            (0, LoadErrorKind::Unknown),
        ];
        for (status, kind) in cases {
            assert_eq!(
                LoadError::from_web_error_status(status, "https://x/".into())
                    .unwrap()
                    .kind,
                kind,
                "status {status}"
            );
        }
        assert!(LoadError::from_web_error_status(14, "https://x/".into()).is_none());
    }

    #[test]
    fn glib_tls_and_cancel() {
        assert_eq!(
            LoadError::from_glib(false, Some(1), "https://x/".into())
                .unwrap()
                .kind,
            LoadErrorKind::TlsError
        );
        assert_eq!(
            LoadError::from_glib(false, None, "https://x/".into())
                .unwrap()
                .kind,
            LoadErrorKind::Unknown
        );
        assert!(LoadError::from_glib(true, None, "https://x/".into()).is_none());
    }

    #[test]
    fn ns_error_maps_tls_faults() {
        let cases = [
            (-1201, Some(TlsFault::Expired)),   // certificate has bad date
            (-1204, Some(TlsFault::Expired)),   // certificate not yet valid
            (-1202, Some(TlsFault::Untrusted)), // certificate untrusted
            (-1203, Some(TlsFault::Untrusted)), // unknown root
            (-1200, Some(TlsFault::Protocol)),  // secure connection failed
            (-1205, Some(TlsFault::Protocol)),  // client certificate rejected
            (-1206, Some(TlsFault::Protocol)),  // client certificate required
            (-1003, None),                      // dns: no tls fault
        ];
        for (code, tls) in cases {
            let err = LoadError::from_ns_error("NSURLErrorDomain", code, "https://x/".into())
                .expect("should surface");
            assert_eq!(err.tls, tls, "code {code}");
        }
    }

    #[test]
    fn web_error_status_maps_tls_faults() {
        let cases = [
            (1, Some(TlsFault::NameMismatch)), // common name incorrect
            (2, Some(TlsFault::Expired)),
            (3, Some(TlsFault::Protocol)), // client certificate errors
            (4, Some(TlsFault::Revoked)),
            (5, Some(TlsFault::Untrusted)), // certificate invalid
            (7, None),                      // timeout: no tls fault
        ];
        for (status, tls) in cases {
            assert_eq!(
                LoadError::from_web_error_status(status, "https://x/".into())
                    .unwrap()
                    .tls,
                tls,
                "status {status}"
            );
        }
    }

    #[test]
    fn gtls_flags_map_by_severity() {
        // G_TLS_CERTIFICATE_* bits: 1 unknown-CA, 2 bad-identity,
        // 4 not-activated, 8 expired, 16 revoked, 32 insecure, 64 generic.
        let cases = [
            (16 | 8 | 1, TlsFault::Revoked), // revoked wins over all
            (8 | 2, TlsFault::Expired),      // expiry wins over identity
            (4, TlsFault::Expired),
            (2 | 1, TlsFault::NameMismatch),
            (1, TlsFault::Untrusted),
            (32, TlsFault::Untrusted),
            (64, TlsFault::Protocol),
            (0, TlsFault::Protocol), // empty flags fail closed
        ];
        for (flags, fault) in cases {
            assert_eq!(
                LoadError::from_glib(false, Some(flags), "https://x/".into())
                    .unwrap()
                    .tls,
                Some(fault),
                "flags {flags:#b}"
            );
        }
    }
}
