//! Per-webview memory readout: the physical footprint of each webview's
//! WebContent process, for the Settings task manager and memory-pressure
//! eviction policies.
//!
//! The pid comes from WebKit's `_webProcessIdentifier` (guarded behind
//! `respondsToSelector`, returning 0 when unavailable); the footprint is
//! `proc_pid_rusage`'s `ri_phys_footprint`, the same metric Activity
//! Monitor's Memory column uses.

use serde::Serialize;
use tauri::{AppHandle, Manager};
use wis_core::WisResult;

#[derive(Debug, Clone, Serialize)]
pub struct WebviewMemory {
    /// Tauri webview label (`tab-<id>`, `widget-panel-<id>`, `overlay-*`,
    /// `peek-page`, `main`); the frontend maps it to a friendly name.
    pub label: String,
    pub pid: i32,
    pub bytes: u64,
}

/// Snapshot every webview's WebContent process footprint.
#[tauri::command]
pub async fn list_webview_memory(app: AppHandle) -> WisResult<Vec<WebviewMemory>> {
    let webviews: Vec<_> = app
        .webviews()
        .iter()
        .map(|(label, wv)| (label.clone(), wv.clone()))
        .collect();

    let mut out = Vec::new();
    for (label, wv) in webviews {
        let (tx, rx) = std::sync::mpsc::channel::<i32>();
        let _ = wv.with_webview(move |w| {
            let pid = web_process_pid(w.inner() as *mut objc2::runtime::AnyObject);
            let _ = tx.send(pid);
        });
        let pid = rx
            .recv_timeout(std::time::Duration::from_millis(500))
            .unwrap_or(0);
        if pid <= 0 {
            continue;
        }
        let Some(bytes) = phys_footprint(pid) else {
            continue;
        };

        out.push(WebviewMemory { label, pid, bytes });
    }

    Ok(out)
}

#[cfg(target_os = "macos")]
fn web_process_pid(wk: *mut objc2::runtime::AnyObject) -> i32 {
    use objc2::{msg_send, sel};

    if wk.is_null() {
        return 0;
    }
    unsafe {
        let responds: bool = msg_send![wk, respondsToSelector: sel!(_webProcessIdentifier)];
        if !responds {
            return 0;
        }

        let pid: i32 = msg_send![wk, _webProcessIdentifier];
        pid
    }
}

#[cfg(not(target_os = "macos"))]
fn web_process_pid(_wk: *mut objc2::runtime::AnyObject) -> i32 {
    0
}

#[cfg(target_os = "macos")]
fn phys_footprint(pid: i32) -> Option<u64> {
    let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
    let ret = unsafe {
        libc::proc_pid_rusage(
            pid,
            libc::RUSAGE_INFO_V4,
            &mut info as *mut libc::rusage_info_v4 as *mut libc::rusage_info_t,
        )
    };

    (ret == 0).then_some(info.ri_phys_footprint)
}

#[cfg(not(target_os = "macos"))]
fn phys_footprint(_pid: i32) -> Option<u64> {
    None
}
