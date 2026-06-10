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
        let remote =
            label.starts_with("tab-") || label.starts_with("widget-panel-") || label == "peek-page";
        if remote {
            apply_fraud_warning(&webview, enabled);
        }
    }
}
