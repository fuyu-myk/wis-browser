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

/// The rule lists each privacy mode attaches, none with Permissive privacy.
pub type RuleSpecs = HashMap<PrivacyMode, Vec<RuleSpec>>;

/// Per-mode specs: the tracker list, then the notice list's network rules as
/// a list of their own. A conversion failure is logged and leaves that list
/// out so a tab still loads. Identifiers hash the input, so a refreshed list
/// gets a new one and WebKit recompiles, while the same list keeps its
/// compiled copy.
pub fn build_rule_specs_from(filter_text: &str, notice_rules: Option<&str>) -> RuleSpecs {
    let notices = notice_rules.and_then(notice_spec);
    [
        PrivacyMode::Strict,
        PrivacyMode::Balanced,
        PrivacyMode::Permissive,
    ]
    .into_iter()
    .map(|mode| {
        let mut specs: Vec<RuleSpec> = tracker_spec(mode, filter_text).into_iter().collect();
        if mode != PrivacyMode::Permissive {
            specs.extend(notices.clone());
        }
        (mode, specs)
    })
    .collect()
}

fn tracker_spec(mode: PrivacyMode, filter_text: &str) -> Option<RuleSpec> {
    match content_blocking_json(&[filter_text], mode) {
        Ok(Some(json)) => Some(RuleSpec {
            identifier: identifier_for(mode_slug(mode), filter_text),
            json,
        }),
        Ok(None) => None,
        Err(err) => {
            log::warn!("content filter: rule generation for {mode:?} failed: {err}");
            None
        }
    }
}

fn notice_spec(rules: &str) -> Option<RuleSpec> {
    match wis_network::content_filter::list_blocking_json(&[rules]) {
        Ok(json) => Some(RuleSpec {
            identifier: identifier_for("notices", rules),
            json,
        }),
        Err(err) => {
            log::warn!("content filter: rule generation for the notice list failed: {err}");
            None
        }
    }
}

/// The list that keeps a Tor or SOCKS workspace's pages off this machine.
pub fn local_spec() -> &'static RuleSpec {
    static SPEC: std::sync::LazyLock<RuleSpec> = std::sync::LazyLock::new(|| {
        let json = wis_network::content_filter::local_blocking_json();
        RuleSpec {
            identifier: identifier_for("local", &json),
            json,
        }
    });
    &SPEC
}

pub const RULE_LIST_PREFIX: &str = "wis-cb-";
/// The local list's identifiers, which [`identifier_for`] gives.
pub const LOCAL_LIST_PREFIX: &str = "wis-cb-local-";

/// Bump when the conversion to WebKit rules changes, so lists compiled by the
/// old conversion are replaced.
const RULES_VERSION: u32 = 2;

/// Hashes the input rather than the JSON: the converter emits each rule's
/// resource types in hash order, so the same list never converts to the same
/// text twice.
fn identifier_for(slug: &str, filter_text: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    RULES_VERSION.hash(&mut hasher);
    filter_text.hash(&mut hasher);
    format!("{RULE_LIST_PREFIX}{slug}-{:016x}", hasher.finish())
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
    use objc2::rc::Id;
    use objc2::runtime::AnyObject;
    use tauri::Manager;

    let app = webview.app_handle().clone();
    let spec = spec.clone();
    webview
        // SAFETY: `with_webview` runs on the main thread with the live
        // WKWebView; it stays retained until the list is added.
        .with_webview(move |w| unsafe {
            let Some(wkwebview) = Id::retain(w.inner() as *mut AnyObject) else {
                log::warn!("content filter: WKWebView pointer was null");
                return;
            };
            resolve(app, spec, move |list| {
                if !list.is_null() {
                    swap_in(&wkwebview, false, &[list]);
                }
            });
        })
        .map_err(|err| format!("with_webview failed: {err}"))
}

/// Replaces Wis's lists on the webview in one step once every spec has
/// resolved, so a page is never left between lists. Extensions' rule lists
/// stay attached.
#[cfg(target_os = "macos")]
pub fn replace_rule_lists<R: tauri::Runtime>(
    webview: &tauri::webview::Webview<R>,
    specs: &[RuleSpec],
) -> Result<(), String> {
    use std::cell::RefCell;
    use std::rc::Rc;

    use objc2::rc::Id;
    use objc2::runtime::AnyObject;
    use tauri::Manager;

    let app = webview.app_handle().clone();
    let specs = specs.to_vec();
    webview
        // SAFETY: `with_webview` runs on the main thread with the live
        // WKWebView; it stays retained until every list has resolved.
        .with_webview(move |w| unsafe {
            let Some(wkwebview) = Id::retain(w.inner() as *mut AnyObject) else {
                return;
            };
            if specs.is_empty() {
                swap_in(&wkwebview, true, &[]);
                return;
            }

            let left = Rc::new(RefCell::new((specs.len(), Vec::<Id<AnyObject>>::new())));
            for spec in specs {
                let left = left.clone();
                let wkwebview = wkwebview.clone();
                resolve(app.clone(), spec, move |list| {
                    let mut left = left.borrow_mut();
                    if let Some(list) = Id::retain(list) {
                        left.1.push(list);
                    }
                    left.0 -= 1;
                    if left.0 == 0 {
                        let lists: Vec<*mut AnyObject> =
                            left.1.iter().map(|l| Id::as_ptr(l).cast_mut()).collect();
                        swap_in(&wkwebview, true, &lists);
                    }
                });
            }
        })
        .map_err(|err| format!("with_webview failed: {err}"))
}

/// Adds `lists` to the webview's user-content controller, after removing
/// every list first when `replace`.
///
/// # Safety
/// Main thread, with `wkwebview` a `WKWebView`.
#[cfg(target_os = "macos")]
unsafe fn swap_in(
    wkwebview: &objc2::runtime::AnyObject,
    replace: bool,
    lists: &[*mut objc2::runtime::AnyObject],
) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    let config: *mut AnyObject = msg_send![wkwebview, configuration];
    let controller: *mut AnyObject = msg_send![config, userContentController];
    if replace {
        let _: () = msg_send![controller, removeAllContentRuleLists];
    }
    for &list in lists {
        let _: () = msg_send![controller, addContentRuleList: list];
    }
}

/// Hands `spec`'s compiled list to `then`, compiling it on a store miss, or a
/// null list when it fails or a later rebuild no longer wants it. The store
/// calls back on the main thread.
///
/// # Safety
/// Main thread.
#[cfg(target_os = "macos")]
unsafe fn resolve<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    spec: RuleSpec,
    then: impl FnOnce(*mut objc2::runtime::AnyObject) + 'static,
) {
    use std::cell::Cell;
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;
    use std::rc::Rc;

    use block2::RcBlock;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    let then = Rc::new(Cell::new(Some(then)));
    let finish = move |list: *mut AnyObject| {
        if let Some(then) = then.take() {
            then(list);
        }
    };

    let store: *mut AnyObject = msg_send![class!(WKContentRuleListStore), defaultStore];
    let Ok(id_c) = CString::new(spec.identifier.as_str()) else {
        log::warn!("content filter: identifier contained NUL");
        finish(std::ptr::null_mut());
        return;
    };
    if store.is_null() {
        log::warn!("content filter: no default WKContentRuleListStore");
        finish(std::ptr::null_mut());
        return;
    }
    // Counts the top-level rule objects for the diagnostic log. Parse failure
    // here just degrades the log; the JSON itself is already known-valid.
    let rule_count = serde_json::from_str::<Vec<serde_json::Value>>(&spec.json)
        .map(|v| v.len())
        .unwrap_or(0);
    let RuleSpec { identifier, json } = spec;

    let lookup_handler = RcBlock::new(move |list: *mut AnyObject, _error: *mut AnyObject| {
        if !still_wanted(&app, &identifier) {
            finish(std::ptr::null_mut());
            return;
        }
        if !list.is_null() {
            finish(list);
            log::info!("content filter: attached '{identifier}' from compiled cache");
            return;
        }

        // Cache miss (first run): compile, which also persists under the
        // identifier for future lookups.
        let (Ok(id_c), Ok(json_c)) = (
            CString::new(identifier.as_str()),
            CString::new(json.as_str()),
        ) else {
            log::warn!("content filter: identifier/JSON contained NUL");
            finish(std::ptr::null_mut());
            return;
        };
        let ns_id: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: id_c.as_ptr()];
        let ns_json: *mut AnyObject =
            msg_send![class!(NSString), stringWithUTF8String: json_c.as_ptr()];
        let store: *mut AnyObject = msg_send![class!(WKContentRuleListStore), defaultStore];
        if store.is_null() {
            finish(std::ptr::null_mut());
            return;
        }

        let identifier = identifier.clone();
        let app = app.clone();
        let finish = finish.clone();
        let compile_handler = RcBlock::new(move |list: *mut AnyObject, error: *mut AnyObject| {
            if !error.is_null() {
                let desc: *mut AnyObject = msg_send![error, localizedDescription];
                let utf8: *const c_char = msg_send![desc, UTF8String];
                let reason = if utf8.is_null() {
                    "<no description>".to_owned()
                } else {
                    CStr::from_ptr(utf8).to_string_lossy().into_owned()
                };
                log::error!("content filter: compile failed: {reason}");
                finish(std::ptr::null_mut());
                return;
            }
            if list.is_null() {
                log::error!("content filter: compile returned nil list with no error");
                finish(std::ptr::null_mut());
                return;
            }
            if !still_wanted(&app, &identifier) {
                finish(std::ptr::null_mut());
                return;
            }

            finish(list);
            log::info!("content filter: compiled + attached '{identifier}' ({rule_count} rules)");
        });

        let _: () = msg_send![
            store,
            compileContentRuleListForIdentifier: ns_id,
            encodedContentRuleList: ns_json,
            completionHandler: &*compile_handler,
        ];
    });

    let ns_id: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: id_c.as_ptr()];
    let _: () = msg_send![
        store,
        lookUpContentRuleListForIdentifier: ns_id,
        completionHandler: &*lookup_handler,
    ];
}

/// Attaches each list; a failure skips that list only.
pub fn attach_rule_lists<R: tauri::Runtime>(
    webview: &tauri::webview::Webview<R>,
    specs: &[RuleSpec],
) -> Result<(), String> {
    specs
        .iter()
        .map(|spec| attach_rule_list(webview, spec))
        .fold(Ok(()), Result::and)
}

/// A compile can finish after a later rebuild has replaced its list.
#[cfg(target_os = "macos")]
fn still_wanted<R: tauri::Runtime>(app: &tauri::AppHandle<R>, identifier: &str) -> bool {
    use tauri::Manager;
    app.try_state::<crate::state::AppState>()
        .is_none_or(|state| state.wis_rule_list_ids().iter().any(|id| id == identifier))
}

/// Remove compiled rule lists left behind by previous filter-list versions.
/// Every Wis identifier is `wis-cb-<mode or notices>-<hash>`; anything with the prefix
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
                if !id.starts_with(RULE_LIST_PREFIX) || keep.contains(&id) {
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

#[cfg(not(target_os = "macos"))]
pub fn attach_rule_list<R: tauri::Runtime>(
    _webview: &tauri::webview::Webview<R>,
    _spec: &RuleSpec,
) -> Result<(), String> {
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn replace_rule_lists<R: tauri::Runtime>(
    _webview: &tauri::webview::Webview<R>,
    _specs: &[RuleSpec],
) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rules with several resource types, which the converter emits in hash
    /// order.
    fn list() -> String {
        (0..50)
            .map(|n| format!("/beacon{n}?$script,image,font,stylesheet,media\n"))
            .collect()
    }

    fn tracker_id(specs: &RuleSpecs, mode: PrivacyMode) -> Option<String> {
        specs[&mode].first().map(|spec| spec.identifier.clone())
    }

    #[test]
    fn the_same_lists_get_the_same_identifier_every_launch() {
        let first = build_rule_specs_from(&list(), None);
        let second = build_rule_specs_from(&list(), None);
        assert_eq!(
            tracker_id(&first, PrivacyMode::Strict),
            tracker_id(&second, PrivacyMode::Strict)
        );
        assert_eq!(
            tracker_id(&first, PrivacyMode::Balanced),
            tracker_id(&second, PrivacyMode::Balanced)
        );
        assert_ne!(
            tracker_id(&first, PrivacyMode::Strict),
            tracker_id(&first, PrivacyMode::Balanced)
        );
        assert_ne!(
            tracker_id(&first, PrivacyMode::Balanced),
            tracker_id(
                &build_rule_specs_from("||other.test^\n", None),
                PrivacyMode::Balanced
            )
        );
    }

    /// WebKit applies a list's exceptions only within that list, so the
    /// notice list's own cannot lift Strict's script block or EasyPrivacy's.
    #[test]
    fn a_notice_list_compiles_on_its_own_beside_the_tracker_list() {
        let trackers = list();
        let alone = build_rule_specs_from(&trackers, None);
        let joined = build_rule_specs_from(
            &trackers,
            Some("||consent.test^\n@@||cdn.test/consent.js\n"),
        );

        for mode in [PrivacyMode::Strict, PrivacyMode::Balanced] {
            let specs = &joined[&mode];
            assert_eq!(specs.len(), 2);
            assert_eq!(tracker_id(&joined, mode), tracker_id(&alone, mode));
            assert!(!specs[0].json.contains("consent"));
            assert!(specs[1].json.contains("consent"));
            assert!(specs[1].identifier.starts_with("wis-cb-notices-"));
        }
        assert_eq!(
            joined[&PrivacyMode::Strict][1].identifier,
            joined[&PrivacyMode::Balanced][1].identifier
        );
        assert!(joined[&PrivacyMode::Permissive].is_empty());
    }
}
