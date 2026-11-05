//! Toasts Rust raises in one window's chrome.

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// In step with `TOAST_REMOTE_EVENT` in lib/toast.ts.
const TOAST_REMOTE_EVENT: &str = "wis-toast-remote";

#[derive(Clone, Serialize)]
struct RemoteToast<'a> {
    window: &'a str,
    message: &'a str,
}

/// Shows `message` in the chrome of the window labelled `window`.
pub fn show(app: &AppHandle, window: &str, message: &str) {
    if let Err(err) = app.emit(TOAST_REMOTE_EVENT, RemoteToast { window, message }) {
        log::warn!("toast for {window}: {err}");
    }
}
