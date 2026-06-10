//! Engine-level content filtering via `WKContentRuleList` (macOS).
//!
//! WebKit applies these rules to requests it makes as the legitimate TLS
//! endpoint, so filtering happens at the URL layer with TLS, HSTS, and
//! certificate pinning intact. The rule JSON is generated from the same
//! filter lists the proxy uses (see `wis_network::content_blocking_json`).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use wis_core::PrivacyMode;
use wis_network::content_blocking_json;

/// A compiled-rule-list spec: a stable identifier (so the WebKit store caches
/// across launches) paired with the content-blocker JSON to compile.
#[derive(Clone)]
pub struct RuleSpec {
    pub identifier: String,
    pub json: String,
}

/// Build per-mode rule specs from the given filter list text. A mode maps to
/// `None` when it applies no engine-level filtering (Permissive). A
/// conversion failure is logged and treated as `None` so a tab still loads.
/// The identifier hashes the JSON so swapping in a refreshed filter list
/// produces a different identifier and forces WebKit to recompile.
pub fn build_rule_specs_from(filter_text: &str) -> HashMap<PrivacyMode, Option<RuleSpec>> {
    [
        PrivacyMode::Strict,
        PrivacyMode::Balanced,
        PrivacyMode::Permissive,
    ]
    .into_iter()
    .map(|mode| (mode, spec_for(mode, filter_text)))
    .collect()
}

fn spec_for(mode: PrivacyMode, filter_text: &str) -> Option<RuleSpec> {
    match content_blocking_json(&[filter_text], mode) {
        Ok(Some(json)) => Some(RuleSpec {
            identifier: identifier_for(mode, &json),
            json,
        }),
        Ok(None) => None,
        Err(err) => {
            log::warn!("content filter: rule generation for {mode:?} failed: {err}");
            None
        }
    }
}

fn identifier_for(mode: PrivacyMode, json: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    json.hash(&mut hasher);
    format!("wis-cb-{}-{:016x}", mode_slug(mode), hasher.finish())
}

fn mode_slug(mode: PrivacyMode) -> &'static str {
    match mode {
        PrivacyMode::Strict => "strict",
        PrivacyMode::Balanced => "balanced",
        PrivacyMode::Permissive => "permissive",
    }
}

/// Attach `spec` to the webview's user-content controller.
///
/// The `WKContentRuleListStore` persists compiled lists by identifier, so the
/// store is asked to **look the identifier up first** and the expensive
/// compile (a full parse of EasyPrivacy-size JSON, hundreds of ms of CPU)
/// runs only on a miss: in practice once per filter-list version, instead of
/// on every tab creation and every launch. The identifier hashes the JSON
/// (see [`build_rule_specs_from`]), so a refreshed list misses naturally.
#[cfg(target_os = "macos")]
pub fn attach_rule_list<R: tauri::Runtime>(
    webview: &tauri::webview::Webview<R>,
    spec: &RuleSpec,
) -> Result<(), String> {
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    use block2::RcBlock;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    let identifier = spec.identifier.clone();
    let json = spec.json.clone();
    // Counts the top-level rule objects for the diagnostic log. Parse failure
    // here just degrades the log; the JSON itself is already known-valid.
    let rule_count = serde_json::from_str::<Vec<serde_json::Value>>(&json)
        .map(|v| v.len())
        .unwrap_or(0);

    /// Attach a compiled list to the webview identified by `webview_addr`.
    /// Runs inside store completion blocks (main thread; the webview outlives
    /// the tab, and the address travels as a usize to keep the block Send).
    unsafe fn attach(webview_addr: usize, list: *mut AnyObject) {
        let wkwebview = webview_addr as *mut AnyObject;
        let config: *mut AnyObject = msg_send![wkwebview, configuration];
        let controller: *mut AnyObject = msg_send![config, userContentController];
        let _: () = msg_send![controller, addContentRuleList: list];
    }

    webview
        .with_webview(move |w| unsafe {
            let wkwebview: *mut AnyObject = w.inner() as *mut AnyObject;
            if wkwebview.is_null() {
                log::warn!("content filter: WKWebView pointer was null");
                return;
            }

            let store: *mut AnyObject = msg_send![class!(WKContentRuleListStore), defaultStore];
            if store.is_null() {
                log::warn!("content filter: no default WKContentRuleListStore");
                return;
            }

            let webview_addr = wkwebview as usize;
            let id_for_lookup = identifier.clone();
            let json_for_compile = json.clone();

            let lookup_handler = RcBlock::new(move |list: *mut AnyObject, _error: *mut AnyObject| {
                if !list.is_null() {
                    attach(webview_addr, list);
                    log::info!(
                        "content filter: attached '{id_for_lookup}' from compiled cache"
                    );
                    return;
                }

                // Cache miss (first run): compile, which also persists under
                // the identifier for future lookups.
                let (Ok(id_c), Ok(json_c)) = (
                    CString::new(id_for_lookup.as_str()),
                    CString::new(json_for_compile.as_str()),
                ) else {
                    log::warn!("content filter: identifier/JSON contained NUL");
                    return;
                };
                let ns_id: *mut AnyObject =
                    msg_send![class!(NSString), stringWithUTF8String: id_c.as_ptr()];
                let ns_json: *mut AnyObject =
                    msg_send![class!(NSString), stringWithUTF8String: json_c.as_ptr()];
                let store: *mut AnyObject =
                    msg_send![class!(WKContentRuleListStore), defaultStore];
                if store.is_null() {
                    return;
                }

                let id_for_log = id_for_lookup.clone();
                let compile_handler =
                    RcBlock::new(move |list: *mut AnyObject, error: *mut AnyObject| {
                        if !error.is_null() {
                            let desc: *mut AnyObject = msg_send![error, localizedDescription];
                            let utf8: *const c_char = msg_send![desc, UTF8String];
                            let reason = if utf8.is_null() {
                                "<no description>".to_owned()
                            } else {
                                CStr::from_ptr(utf8).to_string_lossy().into_owned()
                            };
                            log::error!("content filter: compile failed: {reason}");
                            return;
                        }
                        if list.is_null() {
                            log::error!("content filter: compile returned nil list with no error");
                            return;
                        }

                        attach(webview_addr, list);
                        log::info!(
                            "content filter: compiled + attached '{id_for_log}' ({rule_count} rules)"
                        );
                    });

                let _: () = msg_send![
                    store,
                    compileContentRuleListForIdentifier: ns_id,
                    encodedContentRuleList: ns_json,
                    completionHandler: &*compile_handler,
                ];
            });

            let Ok(id_c) = CString::new(identifier.as_str()) else {
                log::warn!("content filter: identifier contained NUL");
                return;
            };
            let ns_id: *mut AnyObject =
                msg_send![class!(NSString), stringWithUTF8String: id_c.as_ptr()];
            let _: () = msg_send![
                store,
                lookUpContentRuleListForIdentifier: ns_id,
                completionHandler: &*lookup_handler,
            ];
        })
        .map_err(|err| format!("with_webview failed: {err}"))
}

/// Remove compiled rule lists left behind by previous filter-list versions.
/// Every Wis identifier is `wis-cb-<mode>-<hash>`; anything with the prefix
/// that isn't in `keep` is stale (the hash changed with a list refresh) and
/// would otherwise accumulate on disk forever. Call once at startup.
#[cfg(target_os = "macos")]
pub fn prune_stale_rule_lists(keep: Vec<String>) {
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    use block2::RcBlock;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    unsafe {
        let store: *mut AnyObject = msg_send![class!(WKContentRuleListStore), defaultStore];
        if store.is_null() {
            return;
        }

        let handler = RcBlock::new(move |identifiers: *mut AnyObject| {
            if identifiers.is_null() {
                return;
            }
            let count: usize = msg_send![identifiers, count];
            for i in 0..count {
                let ns_id: *mut AnyObject = msg_send![identifiers, objectAtIndex: i];
                if ns_id.is_null() {
                    continue;
                }
                let utf8: *const c_char = msg_send![ns_id, UTF8String];
                if utf8.is_null() {
                    continue;
                }
                let id = CStr::from_ptr(utf8).to_string_lossy().into_owned();
                if !id.starts_with("wis-cb-") || keep.contains(&id) {
                    continue;
                }

                let Ok(id_c) = CString::new(id.as_str()) else {
                    continue;
                };
                let ns_remove: *mut AnyObject =
                    msg_send![class!(NSString), stringWithUTF8String: id_c.as_ptr()];
                let store: *mut AnyObject = msg_send![class!(WKContentRuleListStore), defaultStore];
                if store.is_null() {
                    return;
                }
                let done = RcBlock::new(move |_error: *mut AnyObject| {});
                let _: () = msg_send![
                    store,
                    removeContentRuleListForIdentifier: ns_remove,
                    completionHandler: &*done,
                ];
                log::info!("content filter: pruned stale compiled list '{id}'");
            }
        });

        let _: () = msg_send![store, getAvailableContentRuleListIdentifiers: &*handler];
    }
}

#[cfg(not(target_os = "macos"))]
pub fn prune_stale_rule_lists(_keep: Vec<String>) {}

/// Remove every attached rule list from the webview (used when a workspace
/// switches to a mode with less or no filtering).
#[cfg(target_os = "macos")]
pub fn clear_rule_lists<R: tauri::Runtime>(
    webview: &tauri::webview::Webview<R>,
) -> Result<(), String> {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    webview
        .with_webview(move |w| unsafe {
            let wkwebview: *mut AnyObject = w.inner() as *mut AnyObject;
            if wkwebview.is_null() {
                return;
            }
            let config: *mut AnyObject = msg_send![wkwebview, configuration];
            let controller: *mut AnyObject = msg_send![config, userContentController];
            let _: () = msg_send![controller, removeAllContentRuleLists];
        })
        .map_err(|err| format!("with_webview failed: {err}"))
}

#[cfg(not(target_os = "macos"))]
pub fn attach_rule_list<R: tauri::Runtime>(
    _webview: &tauri::webview::Webview<R>,
    _spec: &RuleSpec,
) -> Result<(), String> {
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn clear_rule_lists<R: tauri::Runtime>(
    _webview: &tauri::webview::Webview<R>,
) -> Result<(), String> {
    Ok(())
}
