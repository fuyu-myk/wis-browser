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

/// A transient failed page load surfaced to the chrome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadError {
    pub kind: LoadErrorKind,
    /// The engine's native error code, kept for diagnostics only.
    pub code: i64,
    /// The URL whose load failed.
    pub url: String,
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

            let kind = match code {
                -1003 | -1006 => LoadErrorKind::DnsFailure, // host not found / DNS lookup failed
                -1004 => LoadErrorKind::ConnectionRefused,  // cannot connect to host
                -1001 => LoadErrorKind::Timeout,
                -1009 => LoadErrorKind::Offline, // not connected to internet
                // TLS/certificate failures (-1200 secure connection failed, plus
                // the -1206..=-1201 certificate-specific band).
                -1206..=-1200 => LoadErrorKind::TlsError,
                _ => LoadErrorKind::Unknown,
            };

            return Some(Self { kind, code, url });
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
        let kind = match status {
            1..=5 => LoadErrorKind::TlsError,
            6 | 9 | 10 | 12 => LoadErrorKind::ConnectionRefused,
            7 => LoadErrorKind::Timeout,
            11 => LoadErrorKind::Offline,
            13 => LoadErrorKind::DnsFailure,
            14 => return None,
            _ => LoadErrorKind::Unknown,
        };

        Some(Self {
            kind,
            code: status as i64,
            url,
        })
    }

    /// Build from a Linux WebKitGTK load failure. `tls` marks the
    /// `load-failed-with-tls-errors` signal; `cancelled` marks a user/app
    /// stop. WebKitGTK's `NetworkError` is coarse (no dependable DNS/timeout
    /// discriminant), so a non-TLS failure surfaces as `Unknown`. Returns
    /// `None` when cancelled.
    pub fn from_glib(cancelled: bool, tls: bool, url: String) -> Option<Self> {
        if cancelled {
            return None;
        }

        Some(Self {
            kind: if tls {
                LoadErrorKind::TlsError
            } else {
                LoadErrorKind::Unknown
            },
            code: 0,
            url,
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
            LoadError::from_glib(false, true, "https://x/".into())
                .unwrap()
                .kind,
            LoadErrorKind::TlsError
        );
        assert_eq!(
            LoadError::from_glib(false, false, "https://x/".into())
                .unwrap()
                .kind,
            LoadErrorKind::Unknown
        );
        assert!(LoadError::from_glib(true, false, "https://x/".into()).is_none());
    }
}
