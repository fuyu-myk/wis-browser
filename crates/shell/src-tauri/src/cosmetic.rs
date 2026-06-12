//! Cosmetic filtering (element hiding) for tab webviews.
//!
//! Network blocking (proxy + `WKContentRuleList`) stops requests; cosmetic
//! filtering hides what is left behind: empty ad slots, placeholders and
//! overlay nags. A document-end user script collects the page's classes and
//! ids (one initial scan, then mutation-batched deltas, capped per message)
//! and posts them with the page URL to a narrow `wisCosmetic` handler. Rust
//! answers from the adblock engine: host-specific hide selectors once per
//! document, plus generic class/id selectors per scan, evaluated back into
//! the page (into the sender's frame, so iframes are covered too) as a CSS
//! append. Installation is skipped for Permissive workspaces.

use std::sync::OnceLock;

use tauri::AppHandle;

static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn set_app_handle(app: AppHandle) {
    let _ = APP.set(app);
}

/// Shared prefix on every per-page injected user script, so the navigation
/// swap can drop the previous page's scripts (start + end) while re-adding
/// every other user script (WebKit only offers removeAllUserScripts).
const INJECT_MARKER_PREFIX: &str = "/*wis-inject";
const START_MARKER: &str = "/*wis-inject-start*/";
const END_MARKER: &str = "/*wis-inject-end*/";

/// Swap the per-page injected scripts for an upcoming navigation: adblock
/// scriptlets and document-start user scripts run before the page's scripts;
/// document-end user scripts run after the DOM parses. Called from the tab
/// webview's `on_navigation` hook (inside WebKit's decide-policy on the main
/// thread), so the scripts are in place before the next document is created.
pub fn swap_scriptlets(app: &tauri::AppHandle, label: &str, url: &str) {
    use tauri::Manager;

    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return;
    }
    let Some(webview) = app.webviews().get(label).cloned() else {
        return;
    };

    let state = app.state::<crate::state::AppState>();
    let scriptlets = {
        let guard = state
            .tracker_blocker
            .read()
            .expect("tracker_blocker poisoned");
        guard.scriptlets_for_page(url)
    };
    let (start_scripts, end_scripts) = state.matching_userscripts(url);

    // Document-start bundle: adblock scriptlets first, then start scripts.
    let mut start = scriptlets;
    for src in &start_scripts {
        start.push_str("\n;(function(){try{");
        start.push_str(src);
        start.push_str("}catch(e){}})();");
    }
    let mut end = String::new();
    for src in &end_scripts {
        end.push_str(";(function(){try{");
        end.push_str(src);
        end.push_str("}catch(e){}})();\n");
    }

    // Empty payloads still swap, clearing the previous page's scripts.
    #[cfg(target_os = "macos")]
    imp::swap_injected_scripts(&webview, &start, &end);

    #[cfg(not(target_os = "macos"))]
    let _ = (webview, start, end);
}

/// Injected page script: collects classes/ids and applies returned CSS.
const COSMETIC_JS: &str = r#"(function () {
  if (window.__wisCosmeticInstalled) return;
  window.__wisCosmeticInstalled = true;
  var MAX_PER_SCAN = 1500;
  var seen = {};
  var sentInitial = false;
  var genStyle = null;
  var hostStyle = null;
  function ensure(node, id) {
    if (!node || !node.isConnected) {
      node = document.createElement('style');
      node.id = id;
      (document.head || document.documentElement).appendChild(node);
    }
    return node;
  }
  window.__wisCosmeticApply = function (payload) {
    try {
      if (!payload) return;
      if (payload.host != null) {
        hostStyle = ensure(hostStyle, 'wis-cosmetic-host');
        hostStyle.textContent = payload.host;
      }
      if (payload.generic) {
        genStyle = ensure(genStyle, 'wis-cosmetic');
        genStyle.appendChild(document.createTextNode(payload.generic + "\n"));
      }
    } catch (e) {}
  };
  function collect() {
    var classes = [];
    var ids = [];
    try {
      var els = document.querySelectorAll('[class],[id]');
      for (var i = 0; i < els.length; i++) {
        if (classes.length >= MAX_PER_SCAN && ids.length >= MAX_PER_SCAN) break;
        var el = els[i];
        if (el.id && !seen['#' + el.id] && ids.length < MAX_PER_SCAN) {
          seen['#' + el.id] = 1;
          ids.push(el.id);
        }
        var cl = el.classList;
        for (var j = 0; j < cl.length; j++) {
          if (!seen['.' + cl[j]] && classes.length < MAX_PER_SCAN) {
            seen['.' + cl[j]] = 1;
            classes.push(cl[j]);
          }
        }
      }
    } catch (e) {}
    if (sentInitial && classes.length === 0 && ids.length === 0) return;
    try {
      if (window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.wisCosmetic) {
        window.webkit.messageHandlers.wisCosmetic.postMessage({
          href: location.href,
          initial: !sentInitial,
          classes: classes,
          ids: ids
        });
        sentInitial = true;
      }
    } catch (e) {}
  }
  collect();
  var pending = null;
  try {
    new MutationObserver(function () {
      if (pending) return;
      pending = setTimeout(function () { pending = null; collect(); }, 700);
    }).observe(document.documentElement, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ['class', 'id']
    });
  } catch (e) {}
  var lastHref = location.href;
  var navTimer = null;
  function onNav() {
    if (location.href === lastHref) return;
    lastHref = location.href;
    if (navTimer) return;
    navTimer = setTimeout(function () {
      navTimer = null;
      try {
        window.webkit.messageHandlers.wisCosmetic.postMessage({
          href: location.href, initial: true, classes: [], ids: []
        });
      } catch (e) {}
    }, 350);
  }
  try {
    ['pushState', 'replaceState'].forEach(function (m) {
      var orig = history[m];
      history[m] = function () { var r = orig.apply(this, arguments); onNav(); return r; };
    });
    window.addEventListener('popstate', onNav);
  } catch (e) {}
})();"#;

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    use objc2::rc::Id;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{
        class, declare_class, msg_send, msg_send_id, mutability, sel, ClassType, DeclaredClass,
    };
    use tauri::Manager;

    use super::{APP, COSMETIC_JS, END_MARKER, INJECT_MARKER_PREFIX, PICKER_JS, START_MARKER};

    declare_class!(
        struct ZapHandler;

        unsafe impl ClassType for ZapHandler {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisZapHandler";
        }

        impl DeclaredClass for ZapHandler {
            type Ivars = ();
        }

        unsafe impl ZapHandler {
            #[method(userContentController:didReceiveScriptMessage:)]
            fn did_receive(&self, _ucc: *mut AnyObject, message: *mut AnyObject) {
                unsafe { handle_zap(message) }
            }
        }
    );

    impl ZapHandler {
        fn new() -> Id<Self> {
            unsafe { msg_send_id![Self::alloc(), init] }
        }
    }

    declare_class!(
        struct CosmeticHandler;

        unsafe impl ClassType for CosmeticHandler {
            type Super = NSObject;
            type Mutability = mutability::InteriorMutable;
            const NAME: &'static str = "WisCosmeticHandler";
        }

        impl DeclaredClass for CosmeticHandler {
            type Ivars = ();
        }

        unsafe impl CosmeticHandler {
            #[method(userContentController:didReceiveScriptMessage:)]
            fn did_receive(&self, _ucc: *mut AnyObject, message: *mut AnyObject) {
                unsafe { handle_message(message) }
            }
        }
    );

    impl CosmeticHandler {
        fn new() -> Id<Self> {
            unsafe { msg_send_id![Self::alloc(), init] }
        }
    }

    unsafe fn ns_string(s: &str) -> *mut AnyObject {
        let c = CString::new(s).unwrap_or_default();
        msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()]
    }

    unsafe fn nsstring_to_string(ns: *mut AnyObject) -> Option<String> {
        if ns.is_null() {
            return None;
        }

        let is_str: bool = msg_send![ns, isKindOfClass: class!(NSString)];
        if !is_str {
            return None;
        }

        let utf8: *const c_char = msg_send![ns, UTF8String];
        if utf8.is_null() {
            return None;
        }

        Some(CStr::from_ptr(utf8).to_string_lossy().into_owned())
    }

    unsafe fn dict_string(dict: *mut AnyObject, key: &str) -> Option<String> {
        let k = ns_string(key);
        let v: *mut AnyObject = msg_send![dict, objectForKey: k];
        nsstring_to_string(v)
    }

    unsafe fn dict_bool(dict: *mut AnyObject, key: &str) -> bool {
        let k = ns_string(key);
        let v: *mut AnyObject = msg_send![dict, objectForKey: k];
        if v.is_null() {
            return false;
        }

        let is_num: bool = msg_send![v, isKindOfClass: class!(NSNumber)];
        if !is_num {
            return false;
        }

        let b: bool = msg_send![v, boolValue];
        b
    }

    unsafe fn dict_string_array(dict: *mut AnyObject, key: &str, cap: usize) -> Vec<String> {
        let k = ns_string(key);
        let arr: *mut AnyObject = msg_send![dict, objectForKey: k];
        if arr.is_null() {
            return Vec::new();
        }

        let is_arr: bool = msg_send![arr, isKindOfClass: class!(NSArray)];
        if !is_arr {
            return Vec::new();
        }

        let count: usize = msg_send![arr, count];
        let mut out = Vec::with_capacity(count.min(cap));
        for i in 0..count.min(cap) {
            let item: *mut AnyObject = msg_send![arr, objectAtIndex: i];
            if let Some(s) = nsstring_to_string(item) {
                out.push(s);
            }
        }

        out
    }

    unsafe fn handle_message(message: *mut AnyObject) {
        if message.is_null() {
            return;
        }

        let body: *mut AnyObject = msg_send![message, body];
        if body.is_null() {
            return;
        }

        let is_dict: bool = msg_send![body, isKindOfClass: class!(NSDictionary)];
        if !is_dict {
            return;
        }

        let Some(href) = dict_string(body, "href") else {
            return;
        };
        if !(href.starts_with("http://") || href.starts_with("https://")) {
            return;
        }

        let initial = dict_bool(body, "initial");
        let classes = dict_string_array(body, "classes", 2000);
        let ids = dict_string_array(body, "ids", 2000);

        let Some(app) = APP.get() else {
            return;
        };

        let blocker = {
            let state = app.state::<crate::state::AppState>();
            let guard = state
                .tracker_blocker
                .read()
                .expect("tracker_blocker poisoned");
            guard.clone()
        };

        let page = blocker.cosmetic_for_page(&href);
        let mut host_css = String::new();
        if initial {
            host_css.push_str(&page.hide_css);
            if let Ok(parsed) = url::Url::parse(&href) {
                if let Some(host) = parsed.host_str() {
                    let state = app.state::<crate::state::AppState>();
                    for selector in state.user_cosmetic_selectors(host) {
                        host_css.push_str(&selector);
                        host_css.push_str("{display:none!important;}\n");
                    }
                }
            }
        }
        let generic_css = if !page.generichide && (!classes.is_empty() || !ids.is_empty()) {
            blocker.cosmetic_generic_css(&classes, &ids, &page.exceptions)
        } else {
            String::new()
        };
        // Nothing to apply: a non-initial scan with no matching generics.
        if !initial && generic_css.is_empty() {
            return;
        }
        let payload = serde_json::json!({
            "host": if initial { Some(host_css) } else { None },
            "generic": if generic_css.is_empty() { None } else { Some(generic_css) },
        });
        let Ok(arg) = serde_json::to_string(&payload) else {
            return;
        };
        let js = ns_string(&format!("window.__wisCosmeticApply({arg});"));

        // Reply into the sender's frame so iframes receive their own CSS;
        // fall back to the main frame when the frame-targeted API is absent.
        let webview: *mut AnyObject = msg_send![message, webView];
        if webview.is_null() {
            return;
        }

        let nil_block: *mut AnyObject = std::ptr::null_mut();
        let frame: *mut AnyObject = msg_send![message, frameInfo];
        let in_frame_sel = sel!(evaluateJavaScript:inFrame:inContentWorld:completionHandler:);
        let supports_frames: bool = msg_send![webview, respondsToSelector: in_frame_sel];
        if supports_frames && !frame.is_null() {
            let world: *mut AnyObject = msg_send![class!(WKContentWorld), pageWorld];
            let _: () = msg_send![
                webview,
                evaluateJavaScript: js,
                inFrame: frame,
                inContentWorld: world,
                completionHandler: nil_block
            ];
        } else {
            let _: () = msg_send![webview, evaluateJavaScript: js, completionHandler: nil_block];
        }
    }

    /// Replace the per-page injected scripts: drop every previously injected
    /// script (shared marker prefix), re-add every other (non-injected) user
    /// script, then add the new document-start and document-end payloads.
    pub fn swap_injected_scripts<R: tauri::Runtime>(
        webview: &tauri::webview::Webview<R>,
        start_payload: &str,
        end_payload: &str,
    ) {
        let start_src = if start_payload.is_empty() {
            String::new()
        } else {
            format!("{START_MARKER}\n{start_payload}")
        };
        let end_src = if end_payload.is_empty() {
            String::new()
        } else {
            format!("{END_MARKER}\n{end_payload}")
        };
        let _ = webview.with_webview(move |w| unsafe {
            let wk: *mut AnyObject = w.inner() as *mut AnyObject;
            if wk.is_null() {
                return;
            }

            let config: *mut AnyObject = msg_send![wk, configuration];
            if config.is_null() {
                return;
            }

            let controller: *mut AnyObject = msg_send![config, userContentController];
            if controller.is_null() {
                return;
            }

            let existing: *mut AnyObject = msg_send![controller, userScripts];
            let count: usize = if existing.is_null() {
                0
            } else {
                msg_send![existing, count]
            };
            let marker = ns_string(INJECT_MARKER_PREFIX);
            let mut keep: Vec<*mut AnyObject> = Vec::with_capacity(count);
            for i in 0..count {
                let script: *mut AnyObject = msg_send![existing, objectAtIndex: i];
                let src: *mut AnyObject = msg_send![script, source];
                let marked: bool = if src.is_null() {
                    false
                } else {
                    msg_send![src, hasPrefix: marker]
                };
                if !marked {
                    let _: *mut AnyObject = msg_send![script, retain];
                    keep.push(script);
                }
            }
            let _: () = msg_send![controller, removeAllUserScripts];
            for script in keep {
                let _: () = msg_send![controller, addUserScript: script];
                let _: () = msg_send![script, release];
            }

            let add = |source: &str, injection_time: isize| {
                if source.is_empty() {
                    return;
                }
                let src = ns_string(source);
                let script: *mut AnyObject = msg_send![class!(WKUserScript), alloc];
                let script: *mut AnyObject = msg_send![
                    script,
                    initWithSource: src,
                    injectionTime: injection_time,
                    forMainFrameOnly: true,
                ];
                let _: () = msg_send![controller, addUserScript: script];
                let _: () = msg_send![script, release];
            };
            add(&start_src, 0); // WKUserScriptInjectionTimeAtDocumentStart
            add(&end_src, 1); // WKUserScriptInjectionTimeAtDocumentEnd
        });
    }

    unsafe fn handle_zap(message: *mut AnyObject) {
        if message.is_null() {
            return;
        }

        let body: *mut AnyObject = msg_send![message, body];
        if body.is_null() {
            return;
        }

        let is_dict: bool = msg_send![body, isKindOfClass: class!(NSDictionary)];
        if !is_dict {
            return;
        }

        let Some(href) = dict_string(body, "href") else {
            return;
        };
        let Some(selector) = dict_string(body, "selector") else {
            return;
        };
        let Some(app) = APP.get() else {
            return;
        };

        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            crate::cosmetic::persist_zap(&app, &href, &selector).await;
        });
    }

    pub fn eval_picker<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
        let _ = webview.with_webview(|w| unsafe {
            let wk: *mut AnyObject = w.inner() as *mut AnyObject;
            if wk.is_null() {
                return;
            }

            let src = ns_string(PICKER_JS);
            let nil_block: *mut AnyObject = std::ptr::null_mut();
            let _: () = msg_send![wk, evaluateJavaScript: src, completionHandler: nil_block];
        });
    }

    /// Install the cosmetic-filtering user script + message handler.
    pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
        let _ = webview.with_webview(|w| unsafe {
            let wk: *mut AnyObject = w.inner() as *mut AnyObject;
            if wk.is_null() {
                return;
            }

            let config: *mut AnyObject = msg_send![wk, configuration];
            if config.is_null() {
                return;
            }

            let controller: *mut AnyObject = msg_send![config, userContentController];
            if controller.is_null() {
                return;
            }

            let src = ns_string(COSMETIC_JS);
            let script: *mut AnyObject = msg_send![class!(WKUserScript), alloc];
            let injection_time: isize = 1; // WKUserScriptInjectionTimeAtDocumentEnd
            let script: *mut AnyObject = msg_send![
                script,
                initWithSource: src,
                injectionTime: injection_time,
                forMainFrameOnly: false,
            ];
            let _: () = msg_send![controller, addUserScript: script];
            let _: () = msg_send![script, release];

            let handler = CosmeticHandler::new();
            let name = ns_string("wisCosmetic");
            let _: () = msg_send![controller, addScriptMessageHandler: &*handler, name: name];

            let zap = ZapHandler::new();
            let zap_name = ns_string("wisZap");
            let _: () = msg_send![controller, addScriptMessageHandler: &*zap, name: zap_name];
        });
    }
}

/// Injected on demand: the element picker overlay. Highlights the element
/// under the cursor, and on click computes a stable-ish CSS selector, hides
/// it locally, and posts `{href, selector}` to the `wisZap` handler. Esc
/// (or right-click) cancels. Self-removing.
const PICKER_JS: &str = r#"(function () {
  if (window.__wisPickerActive) {
    if (window.__wisPickerCancel) window.__wisPickerCancel();
    return;
  }
  window.__wisPickerActive = true;
  var box = document.createElement('div');
  box.style.cssText = 'position:fixed;z-index:2147483647;pointer-events:none;background:rgba(107,119,248,0.25);border:1px solid rgba(107,119,248,0.9);border-radius:3px;transition:all 40ms ease;';
  document.documentElement.appendChild(box);
  var current = null;
  function uniqueFor(sel, target) {
    try {
      var m = document.querySelectorAll(sel);
      return m.length === 1 && m[0] === target;
    } catch (_) { return false; }
  }
  function segFor(node) {
    var seg = node.tagName.toLowerCase();
    var cls = (node.className && node.className.baseVal !== undefined)
      ? node.className.baseVal : node.className;
    if (typeof cls === 'string') {
      var stable = cls.split(/\s+/).filter(function (c) {
        return c && c.length < 40 && /^[A-Za-z_][-\w]*$/.test(c);
      }).slice(0, 2);
      if (stable.length) seg += '.' + stable.join('.');
    }
    if (node.parentElement) {
      var sibs = Array.prototype.filter.call(node.parentElement.children, function (c) {
        return c.tagName === node.tagName;
      });
      if (sibs.length > 1) seg += ':nth-of-type(' + (sibs.indexOf(node) + 1) + ')';
    }
    return seg;
  }
  function selectorFor(el) {
    if (!el || el === document.body || el === document.documentElement) return null;
    if (el.id && /^[A-Za-z_][-\w]*$/.test(el.id) && uniqueFor('#' + el.id, el)) {
      return '#' + el.id;
    }
    var parts = [];
    var node = el;
    var depth = 0;
    while (node && node.nodeType === 1 && node !== document.body && depth < 8) {
      if (node.id && /^[A-Za-z_][-\w]*$/.test(node.id)) {
        parts.unshift('#' + node.id);
      } else {
        parts.unshift(segFor(node));
      }
      var candidate = parts.join(' > ');
      if (uniqueFor(candidate, el)) return candidate;
      if (parts[0].charAt(0) === '#') break; // an id ancestor won't narrow further
      node = node.parentElement;
      depth++;
    }
    return null;
  }
  function move(e) {
    var el = document.elementFromPoint(e.clientX, e.clientY);
    if (!el || el === box) return;
    current = el;
    var r = el.getBoundingClientRect();
    box.style.left = r.left + 'px';
    box.style.top = r.top + 'px';
    box.style.width = r.width + 'px';
    box.style.height = r.height + 'px';
  }
  function teardown() {
    window.__wisPickerActive = false;
    window.__wisPickerCancel = null;
    box.remove();
    document.removeEventListener('mousemove', move, true);
    document.removeEventListener('click', click, true);
    document.removeEventListener('keydown', key, true);
    document.removeEventListener('contextmenu', cancel, true);
  }
  window.__wisPickerCancel = teardown;
  function click(e) {
    e.preventDefault();
    e.stopPropagation();
    var sel = selectorFor(current);
    if (sel) {
      try { current.style.setProperty('display', 'none', 'important'); } catch (_) {}
      try {
        window.webkit.messageHandlers.wisZap.postMessage({ href: location.href, selector: sel });
      } catch (_) {}
    }
    teardown();
  }
  function key(e) { if (e.key === 'Escape') { e.preventDefault(); teardown(); } }
  function cancel(e) { e.preventDefault(); teardown(); }
  document.addEventListener('mousemove', move, true);
  document.addEventListener('click', click, true);
  document.addEventListener('keydown', key, true);
  document.addEventListener('contextmenu', cancel, true);
})();"#;

/// Start the element picker in a tab webview by evaluating the picker JS.
pub fn start_picker<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    #[cfg(target_os = "macos")]
    imp::eval_picker(webview);

    #[cfg(not(target_os = "macos"))]
    let _ = webview;
}

/// Install the cosmetic-filtering channel on a tab webview. No-op off macOS.
pub fn install<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    #[cfg(target_os = "macos")]
    imp::install(webview);

    #[cfg(not(target_os = "macos"))]
    let _ = webview;
}

/// Persist a zapped selector for the page's host: append it to the settings
/// map (deduped), refresh the sync read-cache, and write through storage.
pub async fn persist_zap(app: &tauri::AppHandle, href: &str, selector: &str) {
    use tauri::Manager;

    let Some(host) = url::Url::parse(href)
        .ok()
        .and_then(|u| u.host_str().map(String::from))
    else {
        return;
    };
    let state = app.state::<crate::state::AppState>();
    let snapshot = {
        let mut browser = state.browser.lock().await;
        let mut rules = browser.settings.user_cosmetic_rules.clone();
        let entry = rules.entry(host).or_default();
        if entry.iter().any(|s| s == selector) {
            return; // already present; nothing to persist
        }
        entry.push(selector.to_string());
        let next = browser.with_settings(wis_core::UserSettingsPatch {
            user_cosmetic_rules: Some(rules),
            ..Default::default()
        });
        *browser = next;
        browser.clone()
    };

    state.refresh_user_cosmetic_cache(snapshot.settings.user_cosmetic_rules.clone());
    state
        .save_coalescer
        .schedule(state.storage.clone(), snapshot);
}
