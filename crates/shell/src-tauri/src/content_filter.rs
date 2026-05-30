//! Engine-level content filtering via `WKContentRuleList` (macOS).
//!
//! WebKit applies these rules to requests it makes as the legitimate TLS
//! endpoint, so filtering happens at the URL layer with TLS, HSTS, and
//! certificate pinning intact. The rule JSON is generated from the same
//! filter lists the proxy uses (see `wis_network::content_blocking_json`).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use wis_core::PrivacyMode;
use wis_network::{content_blocking_json, filter_lists::DEFAULT_TRACKER_LIST};

/// A compiled-rule-list spec: a stable identifier (so the WebKit store caches
/// across launches) paired with the content-blocker JSON to compile.
#[derive(Clone)]
pub struct RuleSpec {
    pub identifier: String,
    pub json: String,
}

/// Build the per-mode rule specs once from the bundled tracker list. A mode
/// maps to `None` when it applies no engine-level filtering (Permissive). A
/// conversion failure is logged and treated as `None` so a tab still loads.
pub fn build_rule_specs() -> HashMap<PrivacyMode, Option<RuleSpec>> {
    [
        PrivacyMode::Strict,
        PrivacyMode::Balanced,
        PrivacyMode::Permissive,
    ]
    .into_iter()
    .map(|mode| (mode, spec_for(mode)))
    .collect()
}

fn spec_for(mode: PrivacyMode) -> Option<RuleSpec> {
    match content_blocking_json(&[DEFAULT_TRACKER_LIST], mode) {
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

/// Compile `spec` and attach it to the webview's user-content controller.
/// Compilation is asynchronous; the WebKit store caches by identifier, so
/// re-attaching an already-compiled spec is cheap.
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

    webview
        .with_webview(move |w| unsafe {
            let wkwebview: *mut AnyObject = w.inner() as *mut AnyObject;
            if wkwebview.is_null() {
                log::warn!("content filter: WKWebView pointer was null");
                return;
            }

            let id_c = CString::new(identifier.as_str()).expect("identifier has no NUL");
            let json_c = CString::new(json.as_str()).expect("rule JSON has no NUL");
            let ns_id: *mut AnyObject =
                msg_send![class!(NSString), stringWithUTF8String: id_c.as_ptr()];
            let ns_json: *mut AnyObject =
                msg_send![class!(NSString), stringWithUTF8String: json_c.as_ptr()];

            let store: *mut AnyObject = msg_send![class!(WKContentRuleListStore), defaultStore];
            if store.is_null() {
                log::warn!("content filter: no default WKContentRuleListStore");
                return;
            }

            // The webview address is moved into the completion block as a
            // usize so the closure stays Send and 'static. The block runs on
            // the main thread; the webview outlives the tab.
            let webview_addr = wkwebview as usize;
            let id_for_log = identifier.clone();

            let handler = RcBlock::new(move |list: *mut AnyObject, error: *mut AnyObject| {
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

                let wkwebview = webview_addr as *mut AnyObject;
                let config: *mut AnyObject = msg_send![wkwebview, configuration];
                let controller: *mut AnyObject = msg_send![config, userContentController];
                let _: () = msg_send![controller, addContentRuleList: list];
                log::info!("content filter: attached '{id_for_log}' ({rule_count} rules)");
            });

            let _: () = msg_send![
                store,
                compileContentRuleListForIdentifier: ns_id,
                encodedContentRuleList: ns_json,
                completionHandler: &*handler,
            ];
        })
        .map_err(|err| format!("with_webview failed: {err}"))
}

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
