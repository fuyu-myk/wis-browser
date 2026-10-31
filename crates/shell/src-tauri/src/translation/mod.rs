//! On-device page translation through Apple's Translation framework (macOS
//! 26.4 and later), reached through the Swift bridge in
//! `swift/Translation.swift`. The parsing and queueing are platform-neutral.

#[cfg(target_os = "macos")]
mod bridge;
pub mod command;
#[cfg(target_os = "macos")]
mod fidelity;
mod language;
mod message;
mod objects;
#[cfg(target_os = "macos")]
mod offer;
#[cfg(target_os = "macos")]
mod page;
mod pieces;
mod readings;
#[cfg(target_os = "macos")]
mod session;

#[cfg(target_os = "macos")]
pub use bridge::{check, translate, Check, Status, Strategy, TranslateError, TranslateRequest};
pub use message::Run;

/// Installs the page channel on a tab webview. No-op off macOS.
pub fn install(webview: &tauri::Webview) {
    #[cfg(target_os = "macos")]
    {
        use tauri::Manager;
        session::set_app_handle(webview.app_handle());
        page::install(webview);
    }

    #[cfg(not(target_os = "macos"))]
    let _ = webview;
}

/// Asks the page just loaded in `webview` what language it is in, so the
/// address bar can offer to translate it, and again whenever it moves to
/// another address without loading. No-op off macOS.
pub fn probe(webview: &tauri::Webview) {
    #[cfg(target_os = "macos")]
    offer::probe(webview);

    #[cfg(not(target_os = "macos"))]
    let _ = webview;
}

/// Ends a tab's translation when its page is closed or navigated away.
/// No-op off macOS.
pub fn forget(label: &str) {
    #[cfg(target_os = "macos")]
    {
        session::stop(label);
        offer::forget(label);
    }

    #[cfg(not(target_os = "macos"))]
    let _ = label;
}
