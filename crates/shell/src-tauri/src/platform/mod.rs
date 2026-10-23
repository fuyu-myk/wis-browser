//! Webview engine glue the features call: per-platform page channels and webview settings.

pub(crate) mod content_world;
#[cfg(target_os = "linux")]
pub(crate) mod gtk_dispatch;
#[cfg(target_os = "macos")]
pub(crate) mod native_webview;
#[cfg(any(windows, target_os = "linux", test))]
pub(crate) mod page_message;
#[cfg(target_os = "macos")]
pub(crate) mod webkit_features;
pub(crate) mod webview_close;
pub(crate) mod webview_prefs;
#[cfg(windows)]
pub(crate) mod win_dispatch;
#[cfg(windows)]
pub(crate) mod win_focus;
#[cfg(windows)]
pub(crate) mod win_webview2;
