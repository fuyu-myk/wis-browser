//! Reads off a tab's `WKWebView` handed over as a raw pointer.

use objc2_06::rc::Retained;
use objc2_06::runtime::AnyObject;
use objc2_06::{msg_send, sel};
use objc2_foundation::{NSString, NSURL};

/// The label Wis stamps on each tab's webview as its
/// `accessibilityIdentifier`: `tab-<uuid>`.
///
/// # Safety
///
/// Main thread only; `webview` is null or a live `WKWebView`.
pub unsafe fn label(webview: *const std::ffi::c_void) -> Option<String> {
    let webview = webview.cast::<AnyObject>().as_ref()?;
    let ident: Option<Retained<NSString>> = msg_send![webview, accessibilityIdentifier];
    ident.map(|ident| ident.to_string())
}

/// The URL of the document on screen. `URL` already shows a redirect's
/// target while WebKit decides on it; `_committedURL` does not.
///
/// # Safety
///
/// Main thread only; `webview` is null or a live `WKWebView`.
pub unsafe fn committed_url(webview: *const std::ffi::c_void) -> Option<String> {
    let webview = webview.cast::<AnyObject>().as_ref()?;
    let committed: bool = msg_send![webview, respondsToSelector: sel!(_committedURL)];
    let url: Option<Retained<NSURL>> = if committed {
        msg_send![webview, _committedURL]
    } else {
        msg_send![webview, URL]
    };
    url.and_then(|url| url.absoluteString())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use objc2_06::runtime::{AnyObject, Bool};
    use objc2_06::{class, sel};

    #[test]
    fn every_send_matches_its_method() {
        let webview = class!(WKWebView);
        webview
            .verify_sel::<(), *mut AnyObject>(sel!(accessibilityIdentifier))
            .unwrap();
        webview.verify_sel::<(), *mut AnyObject>(sel!(URL)).unwrap();
        webview
            .verify_sel::<(), *mut AnyObject>(sel!(_committedURL))
            .unwrap();
        webview.verify_sel::<(), Bool>(sel!(isLoading)).unwrap();
    }
}
