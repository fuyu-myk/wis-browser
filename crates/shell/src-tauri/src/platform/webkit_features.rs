//! WebKit's internal feature flags (SPI), set before a webview exists.

use std::ffi::CStr;
use std::os::raw::c_char;

use objc2_06::msg_send;
use objc2_06::runtime::{AnyClass, AnyObject};
use objc2_web_kit::{WKPreferences, WKWebViewConfiguration};

/// WebKit draws pages at 60 fps on faster displays while this is on.
pub(crate) const FRAME_RATE_CAP: &str = "PreferPageRenderingUpdatesNear60FPSEnabled";

/// Wis's own surfaces draw at the display's full rate; web pages keep
/// WebKit's 60 fps cap. Pages get their workspace's hardening here too.
pub fn install_configuration_hook() {
    wry::set_configuration_hook(Box::new(|label, config| {
        // SAFETY: wry calls this on the main thread with the live
        // WKWebViewConfiguration the webview is about to be created from.
        let config = unsafe { &*config.cast::<WKWebViewConfiguration>() };
        // SAFETY: the same call, on the main thread, before the webview exists.
        unsafe { crate::protection::hardening::apply(label, config) };
        if !is_own_surface(label) {
            return;
        }
        // SAFETY: as above.
        let missing = unsafe { disable(&config.preferences(), &[FRAME_RATE_CAP]) };
        if !missing.is_empty() {
            log::warn!("WebKit feature {FRAME_RATE_CAP} is gone; {label} stays at 60 fps");
        }
    }));
}

fn is_own_surface(label: &str) -> bool {
    label == "main" || label.starts_with("win-") || label.starts_with("overlay-")
}

/// Turns off each named feature and returns those this WebKit lacks.
pub(crate) unsafe fn disable(prefs: &WKPreferences, names: &[&'static str]) -> Vec<&'static str> {
    let mut missing = names.to_vec();
    let Some(class) = AnyClass::get(c"WKPreferences") else {
        return missing;
    };
    let features: *mut AnyObject = msg_send![class, _features];
    if features.is_null() {
        return missing;
    }

    let count: usize = msg_send![features, count];
    for i in 0..count {
        let feature: *mut AnyObject = msg_send![features, objectAtIndex: i];
        let key: *mut AnyObject = msg_send![feature, key];
        if key.is_null() {
            continue;
        }

        let utf8: *const c_char = msg_send![key, UTF8String];
        if utf8.is_null() {
            continue;
        }
        let key = CStr::from_ptr(utf8).to_bytes();
        if let Some(at) = missing.iter().position(|name| name.as_bytes() == key) {
            let _: () = msg_send![prefs, _setEnabled: false, forFeature: feature];
            missing.remove(at);
        }
    }

    missing
}

#[cfg(test)]
mod tests {
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, sel};

    use super::{c_char, is_own_surface};

    type Obj = *mut AnyObject;

    #[test]
    fn only_wis_surfaces_lose_the_frame_rate_cap() {
        for label in ["main", "win-4", "overlay-main-palette"] {
            assert!(is_own_surface(label), "{label}");
        }
        for label in [
            "tab-1",
            "widget-panel-2",
            "peek-page",
            "wis-fetch-3",
            "video-popout",
        ] {
            assert!(!is_own_surface(label), "{label}");
        }
    }

    /// A wrong message type aborts at runtime only.
    #[test]
    fn every_send_matches_its_method() {
        let prefs = class!(WKPreferences);
        prefs
            .metaclass()
            .verify_sel::<(), Obj>(sel!(_features))
            .unwrap();
        prefs
            .verify_sel::<(Bool, Obj), ()>(sel!(_setEnabled:forFeature:))
            .unwrap();
        class!(_WKFeature).verify_sel::<(), Obj>(sel!(key)).unwrap();
        class!(NSArray)
            .verify_sel::<(), usize>(sel!(count))
            .unwrap();
        class!(NSArray)
            .verify_sel::<(usize,), Obj>(sel!(objectAtIndex:))
            .unwrap();
        class!(NSString)
            .verify_sel::<(), *const c_char>(sel!(UTF8String))
            .unwrap();
    }
}
