//! Cookies & site-data management via WebKit's `WKWebsiteDataStore`.
//!
//! Each non-private workspace gets its own persistent data store, keyed by the
//! workspace UUID (see `create_tab_webview`'s `data_store_identifier`). That
//! store again is reached again with `+[WKWebsiteDataStore dataStoreForIdentifier:]`
//! so cookies/cache/local-storage can be wiped without needing a live webview.
//!
//! The WebKit data-store APIs are asynchronous (completion handlers) and must
//! run on the main thread, so each command hops onto the main thread, kicks off
//! the work, and bridges the completion block back to the awaiting command
//! through a oneshot channel.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use wis_core::{WisResult, WorkspaceId};

#[cfg(any(target_os = "macos", windows))]
use wis_core::WisError;

use crate::state::AppState;

/// A persisted session cookie (one with no expiry, which WebKit keeps only in
/// memory). This is snapshotted to storage and re-injected on launch so logins
/// survive a restart. Only the fields needed to recreate the cookie are kept.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
}

/// One stored-site entry: a domain (WebKit data-record display name) holding
/// cookies and/or other site data in a workspace's store.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SiteDataEntry {
    pub domain: String,
}

/// List the domains that have stored site data (cookies, cache, storage) in a
/// workspace's persistent store, sorted alphabetically. Private workspaces have
/// no persistent store, so this is empty for them.
#[tauri::command]
pub async fn list_site_data(
    app: AppHandle,
    workspace_id: WorkspaceId,
) -> WisResult<Vec<SiteDataEntry>> {
    #[cfg(target_os = "macos")]
    {
        let bytes = *workspace_id.as_uuid().as_bytes();
        let domains = list_site_data_mac(&app, bytes)
            .await
            .map_err(|message| WisError::InvalidInput { message })?;
        Ok(domains
            .into_iter()
            .map(|domain| SiteDataEntry { domain })
            .collect())
    }

    // WebView2 has no by-domain site-data enumeration like `WKWebsiteDataStore`,
    // so Windows lists the cookie domains; cache/storage are wiped wholesale by
    // `clear_site_data`.
    #[cfg(windows)]
    {
        let domains = imp_win::list(&app, workspace_id)
            .await
            .map_err(|message| WisError::InvalidInput { message })?;
        Ok(domains
            .into_iter()
            .map(|domain| SiteDataEntry { domain })
            .collect())
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = (app, workspace_id);
        Ok(Vec::new())
    }
}

/// Remove all stored site data (cookies, cache, storage) for a single `domain`
/// in a workspace's store.
#[tauri::command]
pub async fn delete_site_data(
    app: AppHandle,
    workspace_id: WorkspaceId,
    domain: String,
) -> WisResult<()> {
    #[cfg(target_os = "macos")]
    {
        let bytes = *workspace_id.as_uuid().as_bytes();
        delete_site_data_mac(&app, bytes, domain)
            .await
            .map_err(|message| WisError::InvalidInput { message })?;
    }

    #[cfg(windows)]
    {
        imp_win::delete(&app, workspace_id, domain)
            .await
            .map_err(|message| WisError::InvalidInput { message })?;
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = (app, workspace_id, domain);

    Ok(())
}

/// Wipe all website data (cookies, cache, local/session storage, IndexedDB, …)
/// for a workspace's persistent store. Private (ephemeral) workspaces keep no
/// persistent data, so callers may simply skip them.
#[tauri::command]
pub async fn clear_site_data(app: AppHandle, workspace_id: WorkspaceId) -> WisResult<()> {
    #[cfg(target_os = "macos")]
    {
        let bytes = *workspace_id.as_uuid().as_bytes();
        clear_site_data_mac(&app, bytes)
            .await
            .map_err(|message| WisError::InvalidInput { message })?;
    }

    #[cfg(windows)]
    {
        imp_win::clear(&app, workspace_id)
            .await
            .map_err(|message| WisError::InvalidInput { message })?;
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = (app, workspace_id);

    Ok(())
}

#[cfg(target_os = "macos")]
async fn clear_site_data_mac(app: &AppHandle, uuid_bytes: [u8; 16]) -> Result<(), String> {
    use block2::RcBlock;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use std::sync::Mutex;

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let tx = Mutex::new(Some(tx));

    app.run_on_main_thread(move || unsafe {
        // NSUUID from the workspace's 16 identifier bytes.
        let uuid: *mut AnyObject = msg_send![class!(NSUUID), alloc];
        let uuid: *mut AnyObject = msg_send![uuid, initWithUUIDBytes: uuid_bytes.as_ptr()];

        let store: *mut AnyObject =
            msg_send![class!(WKWebsiteDataStore), dataStoreForIdentifier: uuid];
        let types: *mut AnyObject = msg_send![class!(WKWebsiteDataStore), allWebsiteDataTypes];
        let since: *mut AnyObject = msg_send![class!(NSDate), distantPast];

        // Completion handler signals the awaiting task. WebKit Block_copy's the
        // block, so it outlives this closure.
        let handler = RcBlock::new(move || {
            if let Ok(mut guard) = tx.lock() {
                if let Some(tx) = guard.take() {
                    let _ = tx.send(());
                }
            }
        });

        let _: () = msg_send![
            store,
            removeDataOfTypes: types,
            modifiedSince: since,
            completionHandler: &*handler,
        ];
    })
    .map_err(|err| format!("run_on_main_thread: {err}"))?;

    rx.await
        .map_err(|_| "site-data clear completion dropped".to_string())
}

/// Read an `NSString*` (or null) into an owned Rust `String`.
#[cfg(target_os = "macos")]
unsafe fn ns_string(ptr: *mut objc2::runtime::AnyObject) -> String {
    use objc2::msg_send;
    if ptr.is_null() {
        return String::new();
    }

    let utf8: *const std::os::raw::c_char = msg_send![ptr, UTF8String];
    if utf8.is_null() {
        return String::new();
    }

    std::ffi::CStr::from_ptr(utf8)
        .to_string_lossy()
        .into_owned()
}

#[cfg(target_os = "macos")]
async fn list_site_data_mac(app: &AppHandle, uuid_bytes: [u8; 16]) -> Result<Vec<String>, String> {
    use block2::RcBlock;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use std::sync::Mutex;

    let (tx, rx) = tokio::sync::oneshot::channel::<Vec<String>>();
    let tx = Mutex::new(Some(tx));

    app.run_on_main_thread(move || unsafe {
        let uuid: *mut AnyObject = msg_send![class!(NSUUID), alloc];
        let uuid: *mut AnyObject = msg_send![uuid, initWithUUIDBytes: uuid_bytes.as_ptr()];
        let store: *mut AnyObject =
            msg_send![class!(WKWebsiteDataStore), dataStoreForIdentifier: uuid];
        let _: () = msg_send![uuid, release];
        let _: *mut AnyObject = msg_send![store, retain];
        let types: *mut AnyObject = msg_send![class!(WKWebsiteDataStore), allWebsiteDataTypes];

        let handler = RcBlock::new(move |records: *mut AnyObject| {
            let mut out: Vec<String> = Vec::new();
            if !records.is_null() {
                let count: usize = msg_send![records, count];
                for i in 0..count {
                    let rec: *mut AnyObject = msg_send![records, objectAtIndex: i];
                    let name: *mut AnyObject = msg_send![rec, displayName];
                    let s = ns_string(name);
                    if !s.is_empty() {
                        out.push(s);
                    }
                }
            }

            out.sort();
            out.dedup();

            let _: () = msg_send![store, release];
            if let Ok(mut guard) = tx.lock() {
                if let Some(tx) = guard.take() {
                    let _ = tx.send(out);
                }
            }
        });

        let _: () = msg_send![store, fetchDataRecordsOfTypes: types, completionHandler: &*handler];
    })
    .map_err(|err| format!("run_on_main_thread: {err}"))?;

    rx.await
        .map_err(|_| "site-data list completion dropped".to_string())
}

#[cfg(target_os = "macos")]
async fn delete_site_data_mac(
    app: &AppHandle,
    uuid_bytes: [u8; 16],
    domain: String,
) -> Result<(), String> {
    use block2::RcBlock;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use std::sync::Mutex;

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let tx = Mutex::new(Some(tx));

    app.run_on_main_thread(move || unsafe {
        let uuid: *mut AnyObject = msg_send![class!(NSUUID), alloc];
        let uuid: *mut AnyObject = msg_send![uuid, initWithUUIDBytes: uuid_bytes.as_ptr()];
        let store: *mut AnyObject =
            msg_send![class!(WKWebsiteDataStore), dataStoreForIdentifier: uuid];
        let _: () = msg_send![uuid, release];
        let _: *mut AnyObject = msg_send![store, retain];
        let types: *mut AnyObject = msg_send![class!(WKWebsiteDataStore), allWebsiteDataTypes];

        let tx = std::rc::Rc::new(tx);

        let fetch_handler = {
            let tx = tx.clone();
            RcBlock::new(move |records: *mut AnyObject| {
                let to_remove: *mut AnyObject = msg_send![class!(NSMutableArray), array];
                if !records.is_null() {
                    let count: usize = msg_send![records, count];
                    for i in 0..count {
                        let rec: *mut AnyObject = msg_send![records, objectAtIndex: i];
                        let name: *mut AnyObject = msg_send![rec, displayName];

                        if ns_string(name) == domain {
                            let _: () = msg_send![to_remove, addObject: rec];
                        }
                    }
                }

                let remove_handler = {
                    let tx = tx.clone();
                    RcBlock::new(move || {
                        let _: () = msg_send![store, release];
                        if let Ok(mut guard) = tx.lock() {
                            if let Some(tx) = guard.take() {
                                let _ = tx.send(());
                            }
                        }
                    })
                };

                let rm_types: *mut AnyObject =
                    msg_send![class!(WKWebsiteDataStore), allWebsiteDataTypes];
                let _: () = msg_send![
                    store,
                    removeDataOfTypes: rm_types,
                    forDataRecords: to_remove,
                    completionHandler: &*remove_handler,
                ];
            })
        };

        let _: () =
            msg_send![store, fetchDataRecordsOfTypes: types, completionHandler: &*fetch_handler];
    })
    .map_err(|err| format!("run_on_main_thread: {err}"))?;

    rx.await
        .map_err(|_| "site-data delete completion dropped".to_string())
}

// ---------------------------------------------------------------------------
// Session-cookie persistence (stay-logged-in across restarts)
// ---------------------------------------------------------------------------

/// Absolute lifetime cap for a persisted session: a restored login auto-expires
/// this many days after it was *first* persisted (ongoing snapshots don't reset
/// it), forcing periodic re-auth so a stolen snapshot is only useful for a
/// bounded window. Stale snapshots are also purged from disk at launch.
pub const SESSION_COOKIE_TTL_DAYS: i64 = 14;

/// Whether the user has opted to persist logins across restarts.
#[cfg(target_os = "macos")]
async fn persistence_enabled(app: &AppHandle) -> bool {
    use tauri::Manager;
    app.state::<AppState>()
        .browser
        .lock()
        .await
        .settings
        .persist_sessions
}

/// Drop every persisted session older than the TTL (and, with `all`, every one)
/// from storage; launch hygiene and the on-disable wipe.
pub async fn purge_persisted_sessions(app: &AppHandle, all: bool) {
    use tauri::Manager;
    let storage = app.state::<AppState>().storage.clone();
    let max_age = if all { 0 } else { SESSION_COOKIE_TTL_DAYS };
    let _ = tokio::task::spawn_blocking(move || storage.purge_stale_session_cookies(max_age)).await;
}

/// Snapshot a workspace's in-memory session cookies and save them to storage.
/// No-op for ephemeral workspaces (they have no persistent store) and off macOS.
pub async fn persist_session_cookies(app: &AppHandle, workspace_id: WorkspaceId) {
    #[cfg(target_os = "macos")]
    {
        use tauri::Manager;
        if !persistence_enabled(app).await {
            return;
        }
        let bytes = *workspace_id.as_uuid().as_bytes();
        let records = match snapshot_session_cookies_mac(app, bytes).await {
            Ok(r) => r,
            Err(err) => {
                log::warn!("session-cookie snapshot for {workspace_id} failed: {err}");
                return;
            }
        };
        if records.is_empty() {
            return;
        }

        // Encrypt before persisting.
        let Some(sealed) = seal_cookies(&records) else {
            return;
        };
        let storage = app.state::<AppState>().storage.clone();
        let _ = tokio::task::spawn_blocking(move || {
            storage.save_session_cookies(workspace_id, &sealed)
        })
        .await;
    }

    #[cfg(not(target_os = "macos"))]
    let _ = (app, workspace_id);
}

/// Restore a workspace's saved session cookies into its WebKit data store.
/// Call before the workspace's first page loads. No-op off macOS / when none.
pub async fn restore_session_cookies(app: &AppHandle, workspace_id: WorkspaceId) {
    #[cfg(target_os = "macos")]
    {
        use tauri::Manager;
        if !persistence_enabled(app).await {
            return;
        }
        let storage = app.state::<AppState>().storage.clone();
        let json = tokio::task::spawn_blocking(move || {
            storage.load_session_cookies(workspace_id, SESSION_COOKIE_TTL_DAYS)
        })
        .await
        .ok()
        .and_then(Result::ok)
        .flatten();
        let Some(sealed) = json else { return };
        // Decrypt the snapshot. A parse/decrypt failure (e.g. a pre-encryption
        // plaintext row, or a missing key) is treated as "nothing to restore".
        let Some(records) = open_cookies(&sealed) else {
            return;
        };
        if records.is_empty() {
            return;
        }

        let bytes = *workspace_id.as_uuid().as_bytes();
        if let Err(err) = restore_session_cookies_mac(app, bytes, records).await {
            log::warn!("session-cookie restore for {workspace_id} failed: {err}");
        }
    }

    #[cfg(not(target_os = "macos"))]
    let _ = (app, workspace_id);
}

/// On-disk sealed form: the AEAD nonce + ciphertext (each a byte array in JSON).
#[cfg(target_os = "macos")]
#[derive(Serialize, Deserialize)]
struct SealedCookies {
    n: Vec<u8>,
    c: Vec<u8>,
}

/// The Keychain-held key that encrypts session cookies, loaded once and cached
/// for the process. `None` if the Keychain is unavailable.
#[cfg(target_os = "macos")]
fn session_cookie_key() -> Option<wis_secrets::MasterKey> {
    use std::sync::OnceLock;
    static KEY: OnceLock<wis_secrets::MasterKey> = OnceLock::new();
    if let Some(k) = KEY.get() {
        return Some(k.clone());
    }

    match wis_secrets::session_key_load_or_create() {
        Ok(k) => {
            let _ = KEY.set(k.clone());
            Some(k)
        }
        Err(err) => {
            log::warn!("session-cookie key unavailable; not persisting cookies: {err}");
            None
        }
    }
}

/// Encrypt the cookie list to the on-disk sealed JSON, or `None` on failure.
#[cfg(target_os = "macos")]
fn seal_cookies(records: &[SessionCookie]) -> Option<String> {
    let key = session_cookie_key()?;
    let plaintext = serde_json::to_vec(records).ok()?;
    let blob = wis_secrets::encrypt(&key, &plaintext).ok()?;
    serde_json::to_string(&SealedCookies {
        n: blob.nonce,
        c: blob.ciphertext,
    })
    .ok()
}

/// Decrypt a sealed snapshot back into the cookie list, or `None` on failure.
#[cfg(target_os = "macos")]
fn open_cookies(sealed: &str) -> Option<Vec<SessionCookie>> {
    let key = session_cookie_key()?;
    let SealedCookies { n, c } = serde_json::from_str(sealed).ok()?;
    let blob = wis_secrets::EncryptedBlob {
        ciphertext: c,
        nonce: n,
    };
    let plaintext = wis_secrets::decrypt(&key, &blob).ok()?;
    serde_json::from_slice(&plaintext).ok()
}

/// Make an autoreleased `NSString*` from a Rust string.
#[cfg(target_os = "macos")]
unsafe fn ns_string_new(s: &str) -> *mut objc2::runtime::AnyObject {
    use objc2::{class, msg_send};
    let Ok(c) = std::ffi::CString::new(s) else {
        return std::ptr::null_mut();
    };

    msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()]
}

#[cfg(target_os = "macos")]
async fn snapshot_session_cookies_mac(
    app: &AppHandle,
    uuid_bytes: [u8; 16],
) -> Result<Vec<SessionCookie>, String> {
    use block2::RcBlock;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use std::sync::Mutex;

    let (tx, rx) = tokio::sync::oneshot::channel::<Vec<SessionCookie>>();
    let tx = Mutex::new(Some(tx));

    app.run_on_main_thread(move || unsafe {
        let uuid: *mut AnyObject = msg_send![class!(NSUUID), alloc];
        let uuid: *mut AnyObject = msg_send![uuid, initWithUUIDBytes: uuid_bytes.as_ptr()];
        let store: *mut AnyObject =
            msg_send![class!(WKWebsiteDataStore), dataStoreForIdentifier: uuid];
        let _: () = msg_send![uuid, release];
        let _: *mut AnyObject = msg_send![store, retain];
        let cookie_store: *mut AnyObject = msg_send![store, httpCookieStore];

        let handler = RcBlock::new(move |cookies: *mut AnyObject| {
            let mut out: Vec<SessionCookie> = Vec::new();
            if !cookies.is_null() {
                let count: usize = msg_send![cookies, count];
                for i in 0..count {
                    let c: *mut AnyObject = msg_send![cookies, objectAtIndex: i];
                    let expires: *mut AnyObject = msg_send![c, expiresDate];
                    if !expires.is_null() {
                        continue;
                    }
                    let name = ns_string(msg_send![c, name]);
                    let value = ns_string(msg_send![c, value]);
                    let domain = ns_string(msg_send![c, domain]);
                    let path = ns_string(msg_send![c, path]);
                    let secure: bool = msg_send![c, isSecure];
                    if name.is_empty() || domain.is_empty() {
                        continue;
                    }

                    out.push(SessionCookie {
                        name,
                        value,
                        domain,
                        path,
                        secure,
                    });
                }
            }

            let _: () = msg_send![store, release];
            if let Ok(mut guard) = tx.lock() {
                if let Some(tx) = guard.take() {
                    let _ = tx.send(out);
                }
            }
        });

        let _: () = msg_send![cookie_store, getAllCookies: &*handler];
    })
    .map_err(|err| format!("run_on_main_thread: {err}"))?;

    rx.await
        .map_err(|_| "session-cookie snapshot completion dropped".to_string())
}

#[cfg(target_os = "macos")]
async fn restore_session_cookies_mac(
    app: &AppHandle,
    uuid_bytes: [u8; 16],
    cookies: Vec<SessionCookie>,
) -> Result<(), String> {
    use block2::RcBlock;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use std::sync::{Arc, Mutex};

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let shared = Arc::new(Mutex::new(Some(tx)));
    let remaining = Arc::new(Mutex::new(cookies.len()));

    app.run_on_main_thread(move || unsafe {
        let uuid: *mut AnyObject = msg_send![class!(NSUUID), alloc];
        let uuid: *mut AnyObject = msg_send![uuid, initWithUUIDBytes: uuid_bytes.as_ptr()];
        let store: *mut AnyObject =
            msg_send![class!(WKWebsiteDataStore), dataStoreForIdentifier: uuid];
        let _: () = msg_send![uuid, release];
        let _: *mut AnyObject = msg_send![store, retain];
        let store_addr = store as usize;
        let cookie_store: *mut AnyObject = msg_send![store, httpCookieStore];

        // Keys equal the documented values of the NSHTTPCookie* property
        // constants, so a plain string-keyed dictionary works with
        // `+cookieWithProperties:`. Expiry is intentionally omitted as it's
        // re-created as a session cookie.
        for ck in &cookies {
            let props: *mut AnyObject = msg_send![class!(NSMutableDictionary), dictionary];
            let set = |k: &str, v: *mut AnyObject| {
                if !v.is_null() {
                    let key = ns_string_new(k);
                    let _: () = msg_send![props, setObject: v, forKey: key];
                }
            };
            set("Name", ns_string_new(&ck.name));
            set("Value", ns_string_new(&ck.value));
            set("Domain", ns_string_new(&ck.domain));
            set(
                "Path",
                ns_string_new(if ck.path.is_empty() { "/" } else { &ck.path }),
            );
            if ck.secure {
                set("Secure", ns_string_new("TRUE"));
            }

            let cookie: *mut AnyObject =
                msg_send![class!(NSHTTPCookie), cookieWithProperties: props];

            let finish = {
                let shared = shared.clone();
                let remaining = remaining.clone();
                move || {
                    let mut left = remaining.lock().expect("cookie counter");
                    *left = left.saturating_sub(1);
                    if *left == 0 {
                        let _: () = msg_send![store_addr as *mut AnyObject, release];
                        if let Ok(mut g) = shared.lock() {
                            if let Some(tx) = g.take() {
                                let _ = tx.send(());
                            }
                        }
                    }
                }
            };

            if cookie.is_null() {
                finish();
                continue;
            }
            let handler = RcBlock::new(finish);
            let _: () = msg_send![cookie_store, setCookie: cookie, completionHandler: &*handler];
        }
    })
    .map_err(|err| format!("run_on_main_thread: {err}"))?;

    rx.await
        .map_err(|_| "session-cookie restore completion dropped".to_string())
}

/// Windows site-data via WebView2. All of a workspace's tabs share one profile;
/// cookies are enumerated through the `ICoreWebView2CookieManager`, and a full
/// clear goes through the profile's `ClearBrowsingDataAll`. Each COM call runs
/// on the UI thread via `with_core_webview2`, bridging its async completion
/// handler back to the awaiting command over a channel.
#[cfg(windows)]
mod imp_win {
    use super::AppState;

    use std::sync::mpsc;
    use std::time::Duration;

    use tauri::{AppHandle, Manager};
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Cookie, ICoreWebView2CookieManager, ICoreWebView2Profile2, ICoreWebView2_13,
        ICoreWebView2_2,
    };
    use webview2_com::{
        ClearBrowsingDataCompletedHandler, CoTaskMemPWSTR, GetCookiesCompletedHandler,
    };
    use windows::core::{Interface, PCWSTR, PWSTR};
    use wis_core::WorkspaceId;

    /// A live tab webview of `workspace_id`, or `None` when the workspace
    /// has no live webview.
    async fn workspace_webview(
        app: &AppHandle,
        workspace_id: WorkspaceId,
    ) -> Option<tauri::webview::Webview<tauri::Wry>> {
        let state = app.state::<AppState>();
        let labels: Vec<String> = {
            let browser = state.browser.lock().await;
            let ws = browser.workspaces.iter().find(|w| w.id == workspace_id)?;
            ws.tab_order
                .iter()
                .map(|id| format!("tab-{}", id.as_uuid()))
                .collect()
        };

        labels
            .iter()
            .find_map(|label| app.webviews().get(label).cloned())
    }

    /// Await a UI-thread completion sent over `rx`, off the async runtime.
    async fn recv<T: Send + 'static>(rx: mpsc::Receiver<T>, timeout: Duration) -> Option<T> {
        tokio::task::spawn_blocking(move || rx.recv_timeout(timeout).ok())
            .await
            .ok()
            .flatten()
    }

    /// A cookie's domain, normalized (leading dot stripped, lowercased).
    fn read_domain(cookie: &ICoreWebView2Cookie) -> Option<String> {
        let mut d = PWSTR::null();
        // SAFETY: `Domain` writes a COM-allocated string that `CoTaskMemPWSTR`
        // then owns and frees; runs on the UI thread inside the handler.
        if unsafe { cookie.Domain(&mut d) }.is_err() {
            return None;
        }

        let s = CoTaskMemPWSTR::from(d).to_string();
        let s = s.trim_start_matches('.').trim().to_ascii_lowercase();

        (!s.is_empty()).then_some(s)
    }

    pub(super) async fn list(
        app: &AppHandle,
        workspace_id: WorkspaceId,
    ) -> Result<Vec<String>, String> {
        let Some(webview) = workspace_webview(app, workspace_id).await else {
            return Ok(Vec::new());
        };
        let (tx, rx) = mpsc::channel::<Vec<String>>();
        crate::win_webview2::with_core_webview2(&webview, move |core| {
            let started = (|| -> windows::core::Result<()> {
                let core2: ICoreWebView2_2 = core.cast()?;
                // SAFETY: on the UI thread via `with_core_webview2`.
                let manager: ICoreWebView2CookieManager = unsafe { core2.CookieManager() }?;
                let tx_ok = tx.clone();
                let handler = GetCookiesCompletedHandler::create(Box::new(move |hr, list| {
                    let mut domains: Vec<String> = Vec::new();
                    if hr.is_ok() {
                        if let Some(list) = list {
                            let mut count: u32 = 0;
                            let _ = unsafe { list.Count(&mut count) };

                            for i in 0..count {
                                if let Ok(cookie) = unsafe { list.GetValueAtIndex(i) } {
                                    if let Some(d) = read_domain(&cookie) {
                                        domains.push(d);
                                    }
                                }
                            }
                        }
                    }

                    let _ = tx_ok.send(domains);
                    Ok(())
                }));

                unsafe { manager.GetCookies(PCWSTR::null(), &handler) }
            })();

            if started.is_err() {
                let _ = tx.send(Vec::new());
            }
        });

        let mut domains = recv(rx, Duration::from_secs(10)).await.unwrap_or_default();
        domains.sort();
        domains.dedup();

        Ok(domains)
    }

    pub(super) async fn delete(
        app: &AppHandle,
        workspace_id: WorkspaceId,
        domain: String,
    ) -> Result<(), String> {
        let Some(webview) = workspace_webview(app, workspace_id).await else {
            return Ok(());
        };

        let target = domain.trim_start_matches('.').trim().to_ascii_lowercase();
        let (tx, rx) = mpsc::channel::<()>();
        crate::win_webview2::with_core_webview2(&webview, move |core| {
            let started = (|| -> windows::core::Result<()> {
                let core2: ICoreWebView2_2 = core.cast()?;
                let manager: ICoreWebView2CookieManager = unsafe { core2.CookieManager() }?;
                let manager_del = manager.clone();
                let tx_done = tx.clone();
                let handler = GetCookiesCompletedHandler::create(Box::new(move |hr, list| {
                    if hr.is_ok() {
                        if let Some(list) = list {
                            let mut count: u32 = 0;
                            let _ = unsafe { list.Count(&mut count) };

                            for i in 0..count {
                                if let Ok(cookie) = unsafe { list.GetValueAtIndex(i) } {
                                    if read_domain(&cookie).as_deref() == Some(target.as_str()) {
                                        // SAFETY: UI thread; cookie is from this list.
                                        let _ = unsafe { manager_del.DeleteCookie(&cookie) };
                                    }
                                }
                            }
                        }
                    }

                    let _ = tx_done.send(());
                    Ok(())
                }));

                unsafe { manager.GetCookies(PCWSTR::null(), &handler) }
            })();

            if started.is_err() {
                let _ = tx.send(());
            }
        });

        let _ = recv(rx, Duration::from_secs(10)).await;
        Ok(())
    }

    pub(super) async fn clear(app: &AppHandle, workspace_id: WorkspaceId) -> Result<(), String> {
        let Some(webview) = workspace_webview(app, workspace_id).await else {
            return Err("open a tab in this workspace to clear its data".to_string());
        };

        let (tx, rx) = mpsc::channel::<bool>();
        crate::win_webview2::with_core_webview2(&webview, move |core| {
            let started = (|| -> windows::core::Result<()> {
                let core13: ICoreWebView2_13 = core.cast()?;
                // SAFETY: on the UI thread via `with_core_webview2`.
                let profile = unsafe { core13.Profile() }?;
                let profile2: ICoreWebView2Profile2 = profile.cast()?;
                let tx_ok = tx.clone();
                let handler = ClearBrowsingDataCompletedHandler::create(Box::new(move |hr| {
                    let _ = tx_ok.send(hr.is_ok());
                    Ok(())
                }));

                unsafe { profile2.ClearBrowsingDataAll(&handler) }
            })();

            if started.is_err() {
                let _ = tx.send(false);
            }
        });

        match recv(rx, Duration::from_secs(15)).await {
            Some(true) => Ok(()),
            _ => Err("clearing site data failed".to_string()),
        }
    }
}
