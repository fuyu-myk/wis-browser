//! The user scripts in a `WKUserContentController` that carry a source marker.

use std::ffi::CString;

use objc2::runtime::AnyObject;
use objc2::{class, msg_send, sel};

/// Remove every user script whose source starts with `prefix`, through the
/// `_removeUserScript:` SPI, which every supported macOS has. Returns how many
/// went, and how many there were.
///
/// `removeAllUserScripts` is no fallback: it would also take Wis's other
/// scripts, and `userScripts` is the controller's live array, so the ones to
/// re-add would already be freed.
///
/// # Safety
/// `controller` must be a live `WKUserContentController`, used on the main
/// thread.
pub unsafe fn remove_marked(controller: *mut AnyObject, prefix: &str) -> (usize, usize) {
    let existing: *mut AnyObject = msg_send![controller, userScripts];
    let count: usize = if existing.is_null() {
        0
    } else {
        msg_send![existing, count]
    };
    let marker = ns_string(prefix);
    let marked: Vec<*mut AnyObject> = (0..count)
        .map(|i| -> *mut AnyObject { msg_send![existing, objectAtIndex: i] })
        .filter(|&script| {
            let src: *mut AnyObject = msg_send![script, source];
            !src.is_null() && msg_send![src, hasPrefix: marker]
        })
        .collect();

    let can_remove: bool = msg_send![controller, respondsToSelector: sel!(_removeUserScript:)];
    if !can_remove {
        log::warn!("user scripts: _removeUserScript: is missing, so {prefix} scripts stay");
        return (0, count);
    }

    for &script in &marked {
        let _: () = msg_send![controller, _removeUserScript: script];
    }

    (marked.len(), count)
}

unsafe fn ns_string(s: &str) -> *mut AnyObject {
    let c = CString::new(s).unwrap_or_default();
    msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()]
}

#[cfg(test)]
mod tests {
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, sel};

    #[test]
    fn every_send_matches_its_method() {
        let controller = class!(WKUserContentController);
        controller
            .verify_sel::<(*mut AnyObject,), ()>(sel!(_removeUserScript:))
            .unwrap();
        controller
            .verify_sel::<(), *mut AnyObject>(sel!(userScripts))
            .unwrap();
        class!(NSArray)
            .verify_sel::<(), usize>(sel!(count))
            .unwrap();
        class!(NSArray)
            .verify_sel::<(usize,), *mut AnyObject>(sel!(objectAtIndex:))
            .unwrap();
        class!(WKUserScript)
            .verify_sel::<(), *mut AnyObject>(sel!(source))
            .unwrap();
        class!(NSString)
            .verify_sel::<(*mut AnyObject,), Bool>(sel!(hasPrefix:))
            .unwrap();
    }
}
