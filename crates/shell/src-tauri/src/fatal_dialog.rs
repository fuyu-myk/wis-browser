//! A blocking error dialog for failures that stop the app from launching.
//!
//! This runs from `setup`, before the event loop. macOS drives `NSAlert`
//! directly; other platforms use `rfd`.

/// Show a modal error and return once the user dismisses it.
///
/// Best-effort by design: if the dialog cannot be built there is nothing left
/// to fall back to.
#[cfg(target_os = "macos")]
pub fn show(title: &str, message: &str) {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    // NSAlertStyleCritical
    const NS_ALERT_STYLE_CRITICAL: usize = 2;

    let Some(title) = nsstring(title) else { return };
    let Some(message) = nsstring(message) else {
        return;
    };

    unsafe {
        let alert: *mut AnyObject = msg_send![class!(NSAlert), new];
        if alert.is_null() {
            return;
        }

        let _: () = msg_send![alert, setAlertStyle: NS_ALERT_STYLE_CRITICAL];
        let _: () = msg_send![alert, setMessageText: title];
        let _: () = msg_send![alert, setInformativeText: message];
        let _: isize = msg_send![alert, runModal];
        let _: () = msg_send![alert, release];
    }
}

#[cfg(target_os = "macos")]
fn nsstring(s: &str) -> Option<*mut objc2::runtime::AnyObject> {
    use objc2::{class, msg_send};

    let c = std::ffi::CString::new(s).ok()?;
    let ns: *mut objc2::runtime::AnyObject =
        unsafe { msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()] };

    (!ns.is_null()).then_some(ns)
}

#[cfg(not(target_os = "macos"))]
pub fn show(title: &str, message: &str) {
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title(title)
        .set_description(message)
        .show();
}
