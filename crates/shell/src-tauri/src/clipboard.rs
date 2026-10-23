//! Native clipboard write for chrome-initiated copies. The chrome webview
//! cannot reliably use the async web clipboard API from a menu-accelerator
//! handler (no user gesture in that webview), so the copy-url keybinding
//! goes through `NSPasteboard` on the main thread instead.

use tauri::AppHandle;
use wis_core::{WisError, WisResult};

/// Strip a link's tracking parameters. See [`wis_core::navigation::link_clean`] for
/// what it will and will not take out.
#[tauri::command]
pub async fn clean_link(url: String) -> wis_core::CleanedLink {
    wis_core::clean_link(&url)
}

/// Copy plain text to the system clipboard.
#[tauri::command]
pub async fn copy_text(app: AppHandle, text: String) -> WisResult<()> {
    let (tx, rx) = std::sync::mpsc::channel::<bool>();
    app.run_on_main_thread(move || {
        let _ = tx.send(write_pasteboard(&text));
    })
    .map_err(|err| WisError::InvalidInput {
        message: format!("clipboard dispatch: {err}"),
    })?;

    let ok = rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err(WisError::InvalidInput {
            message: "clipboard write failed".to_string(),
        })
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

#[cfg(target_os = "macos")]
fn write_pasteboard(text: &str) -> bool {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    unsafe {
        let Some(ns) = nsstring(text) else {
            return false;
        };
        let Some(ty) = nsstring("public.utf8-plain-text") else {
            return false;
        };

        let pb: *mut AnyObject = msg_send![class!(NSPasteboard), generalPasteboard];
        if pb.is_null() {
            return false;
        }

        let _: isize = msg_send![pb, clearContents];
        let ok: bool = msg_send![pb, setString: ns, forType: ty];
        ok
    }
}

// Windows and Linux have no menu-handler-gesture restriction, but the chrome
// webview's async web clipboard is still awkward from an accelerator, so write
// natively via arboard.
#[cfg(not(target_os = "macos"))]
fn write_pasteboard(text: &str) -> bool {
    match arboard::Clipboard::new() {
        Ok(mut clipboard) => clipboard.set_text(text.to_string()).is_ok(),
        Err(_) => false,
    }
}
