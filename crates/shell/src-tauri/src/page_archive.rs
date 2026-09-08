//! Save the loaded page as a complete single-file archive.
//!
//! The archive of the document is obtained from the engine. The file opens
//! offline with its images, stylesheets and fonts intact.
//!
//! The format is the engine's own, so it differs by platform: `.webarchive`
//! from WebKit on macOS, `.mhtml` from WebKitGTK and from Chromium's snapshot
//! on Linux and Windows.
//!
//! The panel runs **before** the archive is produced as WebKitGTK writes
//! straight to a destination and needs it up front.

use std::time::Duration;

use tauri::{AppHandle, Manager};
use wis_core::{TabId, WisError, WisResult};

/// Time limit for the archive operation.
const ARCHIVE_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_ARCHIVE_BYTES: usize = 256 * 1024 * 1024;
/// Used when a title and a URL both sanitise away to nothing.
const FALLBACK_STEM: &str = "page";

/// The extension the engine on this platform produces.
pub(crate) const ARCHIVE_EXTENSION: &str = if cfg!(target_os = "macos") {
    "webarchive"
} else {
    "mhtml"
};

fn invalid(message: impl Into<String>) -> WisError {
    WisError::InvalidInput {
        message: message.into(),
    }
}

/// A save-panel filename for a page, from its title, then the URL's last path
/// segment, then its host.
///
/// The extension goes on before sanitising, so a title the sanitiser consumes
/// takes the extension with it and is caught rather than saved unlabelled.
fn suggested_name(title: &str, url: &str) -> String {
    let from_url = || {
        let parsed = url::Url::parse(url).ok()?;
        let segment = parsed
            .path_segments()
            .and_then(|mut segments| segments.rfind(|segment| !segment.is_empty()))
            .map(str::to_string);

        segment.or_else(|| parsed.host_str().map(str::to_string))
    };

    let stem = Some(title.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(from_url)
        .unwrap_or_default();
    let stem: String = stem.chars().take(120).collect();

    let named = wis_core::sanitize_filename(&format!("{stem}.{ARCHIVE_EXTENSION}"));
    if named.ends_with(&format!(".{ARCHIVE_EXTENSION}")) {
        named
    } else {
        format!("{FALLBACK_STEM}.{ARCHIVE_EXTENSION}")
    }
}

/// Obtains the save location from the save panel, then have the engine
/// write it. `None` when the panel is cancelled.
#[tauri::command]
pub async fn save_page_archive(
    app: AppHandle,
    id: TabId,
    title: String,
    url: String,
) -> WisResult<Option<String>> {
    let label = format!("tab-{}", id.as_uuid());
    let webview = app
        .webviews()
        .get(&label)
        .cloned()
        .ok_or_else(|| invalid(format!("no webview labelled {label}")))?;

    let name = suggested_name(&title, &url);
    let panel_app = app.clone();
    let chosen =
        tokio::task::spawn_blocking(move || crate::panels::run_save_panel(&panel_app, &name))
            .await
            .map_err(|err| invalid(format!("save panel: {err}")))??;
    let Some(path) = chosen else {
        return Ok(None);
    };

    imp::write_archive(&webview, &path).await?;

    Ok(Some(path.to_string_lossy().into_owned()))
}

/// Await a channel value off the async runtime, so the worker is not blocked
/// while the engine builds the archive.
async fn recv_archive<T: Send + 'static>(rx: std::sync::mpsc::Receiver<T>) -> Option<T> {
    tokio::task::spawn_blocking(move || rx.recv_timeout(ARCHIVE_TIMEOUT).ok())
        .await
        .ok()
        .flatten()
}

/// WebKit hands back the archive as bytes, so this writes them itself.
#[cfg(target_os = "macos")]
mod imp {
    use super::{invalid, recv_archive, MAX_ARCHIVE_BYTES};
    use block2::RcBlock;
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use std::path::Path;
    use std::sync::mpsc;
    use wis_core::WisResult;

    pub(super) async fn write_archive<R: tauri::Runtime>(
        webview: &tauri::webview::Webview<R>,
        path: &Path,
    ) -> WisResult<()> {
        let (tx, rx) = mpsc::channel::<Option<Vec<u8>>>();
        webview
            .with_webview(move |w| unsafe {
                let wk = w.inner() as *mut AnyObject;
                if wk.is_null() {
                    let _ = tx.send(None);
                    return;
                }

                // Exactly one of the two arguments is nil on every call, so
                // both are taken as raw pointers.
                let block = RcBlock::new(move |data: *mut AnyObject, _err: *mut AnyObject| {
                    let _ = tx.send(archive_bytes(data));
                });
                let _: () = msg_send![wk, createWebArchiveDataWithCompletionHandler: &*block];
            })
            .map_err(|err| invalid(format!("with_webview failed: {err}")))?;

        let bytes = recv_archive(rx)
            .await
            .flatten()
            .ok_or_else(|| invalid("the page could not be archived"))?;

        tokio::fs::write(path, &bytes)
            .await
            .map_err(|err| invalid(format!("writing {}: {err}", path.display())))
    }

    /// Copy an `NSData` out, refusing one that is implausibly large.
    unsafe fn archive_bytes(data: *mut AnyObject) -> Option<Vec<u8>> {
        if data.is_null() {
            return None;
        }

        let length: usize = msg_send![data, length];
        if length == 0 || length > MAX_ARCHIVE_BYTES {
            return None;
        }

        let bytes: *const std::ffi::c_void = msg_send![data, bytes];
        if bytes.is_null() {
            return None;
        }

        Some(std::slice::from_raw_parts(bytes.cast::<u8>(), length).to_vec())
    }
}

/// WebKitGTK writes the destination itself, so no bytes cross back.
#[cfg(target_os = "linux")]
mod imp {
    use super::{invalid, recv_archive};
    use std::path::Path;
    use std::sync::mpsc;
    use wis_core::WisResult;

    pub(super) async fn write_archive<R: tauri::Runtime>(
        webview: &tauri::webview::Webview<R>,
        path: &Path,
    ) -> WisResult<()> {
        let (tx, rx) = mpsc::channel::<bool>();
        let file = gtk::gio::File::for_path(path);
        webview
            .with_webview(move |w| {
                use webkit2gtk::{SaveMode, WebViewExt};
                w.inner().save_to_file(
                    &file,
                    SaveMode::Mhtml,
                    None::<&gtk::gio::Cancellable>,
                    move |result| {
                        let _ = tx.send(result.is_ok());
                    },
                );
            })
            .map_err(|err| invalid(format!("with_webview failed: {err}")))?;

        match recv_archive(rx).await {
            Some(true) => Ok(()),
            _ => Err(invalid("the page could not be archived")),
        }
    }
}

/// Chromium's snapshot is the same one its own save uses. It arrives as JSON,
/// so the MHTML is read back out of it.
#[cfg(windows)]
mod imp {
    use super::{invalid, recv_archive, MAX_ARCHIVE_BYTES};
    use std::path::Path;
    use std::sync::mpsc;
    use wis_core::WisResult;

    pub(super) async fn write_archive<R: tauri::Runtime>(
        webview: &tauri::webview::Webview<R>,
        path: &Path,
    ) -> WisResult<()> {
        let (tx, rx) = mpsc::channel::<Option<String>>();
        crate::win_webview2::with_core_webview2(webview, move |core| {
            let sender = tx.clone();
            let handler = webview2_com::CallDevToolsProtocolMethodCompletedHandler::create(
                Box::new(move |result: windows::core::Result<()>, json: String| {
                    let _ = sender.send(result.is_ok().then_some(json));
                    Ok(())
                }),
            );
            // SAFETY: dispatched to the UI thread by `with_core_webview2`.
            let called = unsafe {
                core.CallDevToolsProtocolMethod(
                    windows::core::w!("Page.captureSnapshot"),
                    windows::core::w!("{}"),
                    &handler,
                )
            };
            if called.is_err() {
                let _ = tx.send(None);
            }
        });

        let json = recv_archive(rx)
            .await
            .flatten()
            .ok_or_else(|| invalid("the page could not be archived"))?;
        let mhtml = snapshot_data(&json).ok_or_else(|| invalid("the archive came back empty"))?;
        if mhtml.len() > MAX_ARCHIVE_BYTES {
            return Err(invalid("that page is too large to archive"));
        }

        tokio::fs::write(path, mhtml.as_bytes())
            .await
            .map_err(|err| invalid(format!("writing {}: {err}", path.display())))
    }

    /// `Page.captureSnapshot` answers `{"data": "<mhtml>"}`.
    fn snapshot_data(json: &str) -> Option<String> {
        let parsed: serde_json::Value = serde_json::from_str(json).ok()?;
        let data = parsed.get("data")?.as_str()?;

        (!data.is_empty()).then(|| data.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extension() -> String {
        format!(".{ARCHIVE_EXTENSION}")
    }

    #[test]
    fn the_title_names_the_file() {
        let name = suggested_name("Kingfisher", "https://en.wikipedia.org/wiki/Kingfisher");
        assert_eq!(name, format!("Kingfisher{}", extension()));
    }

    #[test]
    fn an_empty_title_falls_back_to_the_path() {
        let name = suggested_name("   ", "https://en.wikipedia.org/wiki/Kingfisher");
        assert_eq!(name, format!("Kingfisher{}", extension()));
    }

    #[test]
    fn a_pathless_url_falls_back_to_the_host() {
        let name = suggested_name("", "https://example.com/");
        assert_eq!(name, format!("example.com{}", extension()));
    }

    #[test]
    fn a_title_the_sanitiser_eats_still_gets_an_extension() {
        let name = suggested_name("...", "not a url");
        assert_eq!(name, format!("{FALLBACK_STEM}{}", extension()));
    }

    #[test]
    fn separators_in_a_title_cannot_escape_the_chosen_directory() {
        let name = suggested_name("../../etc/passwd", "https://example.com/");
        assert!(!name.contains('/'), "{name}");
        assert!(!name.contains('\\'), "{name}");
    }

    #[test]
    fn a_headline_length_title_is_trimmed_rather_than_refused() {
        let name = suggested_name(&"a".repeat(400), "https://example.com/");
        assert!(name.ends_with(&extension()), "{name}");
        assert!(name.len() < 200, "still {} bytes", name.len());
    }
}
