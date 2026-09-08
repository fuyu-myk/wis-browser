//! Open the web inspector on a tab.
//!
//! Tauri compiles `Webview::open_devtools` out past `debug_assertions` unless
//! `tauri/devtools` is on, so the workspace enables that feature. Every gate
//! runs a debug build.
//!
//! Open, never toggle. `close_devtools` has an empty body on Windows and
//! `is_devtools_open` always answers false there, so a toggle would be wrong.
//! `is_devtools_open` also blocks on an event-loop round trip, which would
//! deadlock if it were ever called from the main thread.
//!
//! No engine offers a public hook for inspecting a particular node, so this
//! opens the inspector and the user picks the node.

use tauri::{AppHandle, Manager};
use wis_core::{TabId, WisError, WisResult};

/// Open the inspector on `id`'s webview.
///
/// A private workspace's webviews are built with devtools off, so this is a
/// no-op there. The menu offers no row in that case.
#[tauri::command]
pub async fn inspect_tab(app: AppHandle, id: TabId) -> WisResult<()> {
    let label = format!("tab-{}", id.as_uuid());
    let webview = app
        .webviews()
        .get(&label)
        .cloned()
        .ok_or_else(|| WisError::InvalidInput {
            message: format!("no webview labelled {label}"),
        })?;

    webview.open_devtools();

    Ok(())
}
