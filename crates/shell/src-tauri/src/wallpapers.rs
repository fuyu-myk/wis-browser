//! Workspace wallpaper store + the `wis-asset` protocol serving it.
//!
//! Imported images live content-addressed under `<app_data>/wallpapers/`
//! as `<sha256>.<ext>` (mirroring the favicon store): re-importing the same
//! file is a no-op and `Background::Image` references only the hash. The
//! boundary is defensive on both sides:
//! - Import: native open panel only (user gesture), format decided by magic
//!   bytes (never the filename), size-capped so a huge GIF can't balloon RAM.
//! - Serve: `wis-asset://wallpaper/<hash>` answers **only chrome surfaces**
//!   (`main` + `overlay-*` labels); the hash is strictly 64 hex chars so a
//!   request can never address anything outside the wallpapers directory.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tauri::http;
use tauri::{AppHandle, Manager, UriSchemeContext, UriSchemeResponder};
use wis_core::{WisError, WisResult};

/// Reject wallpapers above this size: the chrome keeps the decoded image
/// resident, and animated GIFs decode every frame.
const MAX_WALLPAPER_BYTES: usize = 20 * 1024 * 1024;

const WALLPAPERS_DIR: &str = "wallpapers";

/// Sniffed image format (extension + mime). Magic bytes only.
fn sniff(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some(("png", "image/png"));
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(("jpg", "image/jpeg"));
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(("gif", "image/gif"));
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some(("webp", "image/webp"));
    }

    None
}

fn wallpapers_dir(app: &AppHandle) -> WisResult<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| WisError::InvalidInput {
            message: format!("app data dir: {err}"),
        })?
        .join(WALLPAPERS_DIR);
    std::fs::create_dir_all(&dir).map_err(|err| WisError::InvalidInput {
        message: format!("create {}: {err}", dir.display()),
    })?;

    Ok(dir)
}

/// Open the native image picker, validate + copy the chosen file into the
/// store, and return its hash. `None` when the user cancels.
#[tauri::command]
pub async fn import_wallpaper(app: AppHandle) -> WisResult<Option<String>> {
    let (tx, rx) = std::sync::mpsc::channel::<Option<PathBuf>>();
    app.run_on_main_thread(move || {
        let _ = tx.send(ask_image());
    })
    .map_err(|err| WisError::InvalidInput {
        message: format!("open panel dispatch: {err}"),
    })?;
    let Some(path) = rx
        .recv_timeout(std::time::Duration::from_secs(300))
        .ok()
        .flatten()
    else {
        return Ok(None);
    };

    let bytes = std::fs::read(&path).map_err(|err| WisError::InvalidInput {
        message: format!("read {}: {err}", path.display()),
    })?;
    if bytes.len() > MAX_WALLPAPER_BYTES {
        return Err(WisError::InvalidInput {
            message: "image is larger than 20 MB".to_string(),
        });
    }
    let Some((ext, _mime)) = sniff(&bytes) else {
        return Err(WisError::InvalidInput {
            message: "unsupported image format (png, jpeg, gif, webp)".to_string(),
        });
    };

    let hash = format!("{:x}", Sha256::digest(&bytes));
    let dir = wallpapers_dir(&app)?;
    let target = dir.join(format!("{hash}.{ext}"));
    if !target.exists() {
        std::fs::write(&target, &bytes).map_err(|err| WisError::InvalidInput {
            message: format!("store wallpaper: {err}"),
        })?;
    }

    Ok(Some(hash))
}

/// `wis-asset://wallpaper/<sha256>` handler. Registered app-wide but answers
/// only chrome surfaces; everything else gets 403.
pub fn handle_asset_request(
    ctx: UriSchemeContext<'_, tauri::Wry>,
    request: http::Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let label = ctx.webview_label();
    if label != "main" && !label.starts_with("overlay-") {
        responder.respond(status(403, "forbidden"));
        return;
    }
    let app = ctx.app_handle().clone();
    let uri = request.uri().clone();

    tauri::async_runtime::spawn_blocking(move || {
        // wis-asset://wallpaper/<hash> parses host = "wallpaper", path = "/<hash>".
        let kind = uri.host().unwrap_or_default();
        let hash = uri.path().trim_start_matches('/');
        if kind != "wallpaper" || hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            responder.respond(status(400, "bad request"));
            return;
        }
        let Ok(dir) = wallpapers_dir(&app) else {
            responder.respond(status(500, "store unavailable"));
            return;
        };
        for ext in ["png", "jpg", "gif", "webp"] {
            let path = dir.join(format!("{hash}.{ext}"));
            if let Ok(bytes) = std::fs::read(&path) {
                responder.respond(ok_image(bytes, &path));
                return;
            }
        }

        responder.respond(status(404, "not found"));
    });
}

fn ok_image(bytes: Vec<u8>, path: &Path) -> http::Response<Vec<u8>> {
    let mime = match path.extension().and_then(|e| e.to_str()) {
        Some("png") => "image/png",
        Some("jpg") => "image/jpeg",
        Some("gif") => "image/gif",
        _ => "image/webp",
    };
    http::Response::builder()
        .status(200)
        .header("Content-Type", mime)
        .header("Cache-Control", "max-age=31536000, immutable")
        .body(bytes)
        .expect("static response")
}

fn status(code: u16, msg: &str) -> http::Response<Vec<u8>> {
    http::Response::builder()
        .status(code)
        .body(msg.as_bytes().to_vec())
        .expect("static response")
}

/// Native open panel for images, main thread only (`runModal`).
#[cfg(target_os = "macos")]
fn ask_image() -> Option<PathBuf> {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    unsafe {
        let panel: *mut AnyObject = msg_send![class!(NSOpenPanel), openPanel];
        if panel.is_null() {
            return None;
        }
        let _: () = msg_send![panel, setCanChooseFiles: true];
        let _: () = msg_send![panel, setCanChooseDirectories: false];
        let _: () = msg_send![panel, setAllowsMultipleSelection: false];
        // Filter by extension; the import still trusts magic bytes only.
        let exts: *mut AnyObject = {
            let mut items: Vec<*mut AnyObject> = Vec::new();
            for ext in ["png", "jpg", "jpeg", "gif", "webp"] {
                let c = CString::new(ext).expect("static ext");
                let ns: *mut AnyObject =
                    msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()];
                items.push(ns);
            }
            msg_send![class!(NSArray), arrayWithObjects: items.as_ptr(), count: items.len()]
        };
        let _: () = msg_send![panel, setAllowedFileTypes: exts];

        let response: isize = msg_send![panel, runModal];
        if response != 1 {
            return None;
        }
        let url: *mut AnyObject = msg_send![panel, URL];
        if url.is_null() {
            return None;
        }
        let path_string: *mut AnyObject = msg_send![url, path];
        if path_string.is_null() {
            return None;
        }
        let utf8: *const c_char = msg_send![path_string, UTF8String];
        if utf8.is_null() {
            return None;
        }

        Some(PathBuf::from(
            CStr::from_ptr(utf8).to_string_lossy().into_owned(),
        ))
    }
}

#[cfg(not(target_os = "macos"))]
fn ask_image() -> Option<PathBuf> {
    None
}
