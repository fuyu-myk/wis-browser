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

use tauri::AppHandle;
use wis_core::{WisError, WisResult, WorkspaceId};

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
        return Ok(domains.into_iter().map(|domain| SiteDataEntry { domain }).collect());
    }

    #[cfg(not(target_os = "macos"))]
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

    #[cfg(not(target_os = "macos"))]
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

    #[cfg(not(target_os = "macos"))]
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

    std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned()
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
