//! Native `WKPreferences` tweaks applied to **remote** webviews (tabs, Peek,
//! widget panels). Local chrome/overlay webviews are never touched.

/// Enable/disable WebKit's fraudulent-website warning (Apple-proxied Safe
/// Browsing) on one webview: a full-page warning before suspected
/// phishing/malware sites, complementing [`wis_core::link_safety`]'s structural
/// heuristics with a real threat database. WKWebView defaults to off; Wis
/// turns it on unless the user opts out (the check performs network lookups).
pub fn apply_fraud_warning<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>, enabled: bool) {
    #[cfg(target_os = "macos")]
    {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;

        let _ = webview.with_webview(move |w| unsafe {
            let wk: *mut AnyObject = w.inner() as *mut AnyObject;
            if wk.is_null() {
                return;
            }
            let config: *mut AnyObject = msg_send![wk, configuration];
            if config.is_null() {
                return;
            }
            let prefs: *mut AnyObject = msg_send![config, preferences];
            if prefs.is_null() {
                return;
            }
            let _: () = msg_send![prefs, setFraudulentWebsiteWarningEnabled: enabled];
        });
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (webview, enabled);
    }
}

/// Re-apply the fraud-warning preference to every live remote webview (tabs,
/// Peek, widget panels), so a settings toggle takes effect without reloads.
pub fn apply_fraud_warning_to_all_remote(app: &tauri::AppHandle, enabled: bool) {
    use tauri::Manager;

    for (label, webview) in app.webviews() {
        if is_remote(&label) {
            apply_fraud_warning(&webview, enabled);
        }
    }
}

/// Enable WebKit's Media Source Extensions + Encrypted Media Extensions on a
/// remote webview. MSE (adaptive streaming) is **off by default** in embedded
/// WKWebView, which breaks video on many sites regardless of DRM; EME exposes
/// the FairPlay key system (`com.apple.fps`) to pages that request it. Both
/// are set through guarded WebKit SPI (`respondsToSelector:` first), so a
/// future WebKit that drops a selector degrades silently rather than crashing.
/// Disabling reduces the (Safari-claiming) fingerprint + DRM surface but
/// breaks adaptive playback on some sites. A full WebKit rebuild is not
/// needed: these flip live, taking effect on the webview's next navigation.
pub fn apply_media_extensions<R: tauri::Runtime>(
    webview: &tauri::webview::Webview<R>,
    enabled: bool,
) {
    #[cfg(target_os = "macos")]
    {
        use objc2::runtime::AnyObject;
        use objc2::{msg_send, sel};

        let _ = webview.with_webview(move |w| unsafe {
            let wk: *mut AnyObject = w.inner() as *mut AnyObject;
            if wk.is_null() {
                return;
            }

            let config: *mut AnyObject = msg_send![wk, configuration];
            if config.is_null() {
                return;
            }

            let prefs: *mut AnyObject = msg_send![config, preferences];
            if prefs.is_null() {
                return;
            }

            // WebKit SPI setters; each is gated so an absent selector is a
            // no-op instead of an unrecognised-selector crash.
            if msg_send![prefs, respondsToSelector: sel!(_setMediaSourceEnabled:)] {
                let _: () = msg_send![prefs, _setMediaSourceEnabled: enabled];
            }
            if msg_send![prefs, respondsToSelector: sel!(_setEncryptedMediaAPIEnabled:)] {
                let _: () = msg_send![prefs, _setEncryptedMediaAPIEnabled: enabled];
            }
            if msg_send![prefs, respondsToSelector: sel!(_setLegacyEncryptedMediaAPIEnabled:)] {
                let _: () = msg_send![prefs, _setLegacyEncryptedMediaAPIEnabled: enabled];
            }
        });
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (webview, enabled);
    }
}

/// Re-apply the media-extensions preference to every live remote webview.
pub fn apply_media_extensions_to_all_remote(app: &tauri::AppHandle, enabled: bool) {
    use tauri::Manager;

    for (label, webview) in app.webviews() {
        if is_remote(&label) {
            apply_media_extensions(&webview, enabled);
        }
    }
}

/// True for the webview labels that render untrusted remote content.
pub(crate) fn is_remote(label: &str) -> bool {
    label.starts_with("tab-")
        || label.starts_with("widget-panel-")
        || label.starts_with("peek-page")
}

/// Leave wry's `window.ipc` global off every webview that loads remote pages:
/// Tauri refuses their calls anyway, and the read-only global breaks a page
/// with one of its own by that name.
pub fn keep_ipc_global_off_remote_pages() {
    wry::set_ipc_global_hook(Box::new(|label| !loads_remote_pages(label)));
}

fn loads_remote_pages(label: &str) -> bool {
    is_remote(label) || label.starts_with("wis-fetch-")
}

/// Keep a page's selection alive when it stops being first responder.
///
/// `WebViewImpl::resignFirstResponder` calls `clearSelection()` on the page
/// unless this is set. With it set the selection survives and WebKit paints
/// it in the unfocused grey, returning to the accent when the pane is
/// focused again.
///
/// `WKWebView` has no setter for it, so the page ref is fetched and the C
/// entry point called. Both are SPI and both are guarded.
///
/// Every navigation resets the flag, so this is armed once a load has
/// finished. Measured: armed before a load, or while one is in flight, the
/// selection is still destroyed on blur; armed after each load, it holds.
pub fn keep_inactive_selection<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    #[cfg(target_os = "macos")]
    {
        use objc2::runtime::AnyObject;
        use objc2::{msg_send, sel};

        #[repr(C)]
        struct OpaqueWKPage {
            _private: [u8; 0],
        }

        unsafe impl objc2::RefEncode for OpaqueWKPage {
            const ENCODING_REF: objc2::Encoding =
                objc2::Encoding::Pointer(&objc2::Encoding::Struct("OpaqueWKPage", &[]));
        }

        extern "C" {
            fn WKPageSetMaintainsInactiveSelection(page: *mut OpaqueWKPage, maintains: bool);
        }

        let _ = webview.with_webview(|w| unsafe {
            let wk: *mut AnyObject = w.inner() as *mut AnyObject;
            if wk.is_null() {
                return;
            }

            let known: bool =
                msg_send![wk, respondsToSelector: sel!(_pageRefForTransitionToWKWebView)];
            if !known {
                return;
            }

            let page: *mut OpaqueWKPage = msg_send![wk, _pageRefForTransitionToWKWebView];
            if page.is_null() {
                return;
            }

            WKPageSetMaintainsInactiveSelection(page, true);
        });
    }

    #[cfg(not(target_os = "macos"))]
    let _ = webview;
}

#[cfg(test)]
mod tests {
    use super::loads_remote_pages;

    #[test]
    fn only_the_chrome_keeps_the_ipc_global() {
        for label in ["tab-1", "widget-panel-2", "peek-page-main", "wis-fetch-3"] {
            assert!(loads_remote_pages(label), "{label}");
        }
        for label in ["main", "window-4", "overlay-main-palette"] {
            assert!(!loads_remote_pages(label), "{label}");
        }
    }
}
