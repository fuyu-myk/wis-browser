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
fn is_remote(label: &str) -> bool {
    label.starts_with("tab-") || label.starts_with("widget-panel-") || label == "peek-page"
}
