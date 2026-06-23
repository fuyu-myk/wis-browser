//! Page capture for the active tab. A capture **copies to the clipboard by
//! default** and is held in [`AppState::last_screenshot`] so the success toast's
//! "Save" button ([`save_last_screenshot`]) can write it to disk without
//! re-capturing.
//!
//! Capture uses WKWebView's `takeSnapshotWithConfiguration:completionHandler:`
//! (macOS 11+). The visible viewport is a nil-config snapshot of the tab
//! webview. Window and region captures composite the chrome webview with the
//! live tab webviews into one window image ([`capture_window`](imp::capture_window));
//! region crops that composite to a rect chosen by a full-window selection overlay.
//! All snapshots run on the main thread via `with_webview`; the async completion
//! bridges back over an mpsc channel.

use std::sync::mpsc;
use std::time::Duration;

use tauri::{AppHandle, Manager, State};
use wis_core::{TabId, WisError, WisResult};

use crate::state::AppState;

/// Cap on full-page height (CSS px). A 1200-wide capture at this height is a
/// ~140 MB bitmap before PNG encoding, which bounds runaway memory on
/// infinite-scroll pages while still covering ordinary long articles.
#[cfg(target_os = "macos")]
const MAX_CAPTURE_HEIGHT: f64 = 30000.0;

/// Which part of the page/window to capture.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotMode {
    /// The visible content of the active tab webview.
    Visible,
    /// A user-dragged rectangle of the active tab.
    Region,
    /// The whole browser window as it appears: the chrome composited with the
    /// live tab webviews, captured without Screen Recording permission.
    Window,
}

/// A crop rectangle in the window's content-area coordinates (top-left origin,
/// logical px), supplied by the region-selection overlay for `Region` capture.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
pub struct CropRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Capture the active tab (`Visible`), the whole window (`Window`), or a
/// window-region crop (`Region`, with `rect` from the selection overlay) and
/// copy it to the clipboard (the default), holding the PNG so the success
/// toast's "Save" button can write it to disk later. Returns `true` when a
/// capture was made. `chrome` is the calling window's own webview, injected by
/// Tauri, used as the base layer for `Window`/`Region` captures.
#[tauri::command]
pub async fn capture_to_clipboard(
    app: AppHandle,
    state: State<'_, AppState>,
    chrome: tauri::Webview,
    id: TabId,
    mode: ScreenshotMode,
    rect: Option<CropRect>,
) -> WisResult<bool> {
    log::info!("capture_to_clipboard: tab={id:?} mode={mode:?} rect={rect:?}");

    #[cfg(target_os = "macos")]
    {
        let png = match mode {
            ScreenshotMode::Visible => {
                let label = format!("tab-{}", id.as_uuid());
                let webview =
                    app.webviews()
                        .get(&label)
                        .cloned()
                        .ok_or_else(|| WisError::InvalidInput {
                            message: format!("no webview labelled {label}"),
                        })?;
                imp::capture(&webview, false).await?
            }
            ScreenshotMode::Region => {
                let r = rect.ok_or_else(|| WisError::InvalidInput {
                    message: "region capture requires a selection rect".to_string(),
                })?;
                imp::capture_window(&app, &chrome, Some((r.x, r.y, r.width, r.height))).await?
            }
            ScreenshotMode::Window => imp::capture_window(&app, &chrome, None).await?,
        };

        if !imp::copy_image_to_pasteboard(&app, png.clone()).await {
            return Err(WisError::InvalidInput {
                message: "copying the screenshot to the clipboard failed".to_string(),
            });
        }
        *state
            .last_screenshot
            .lock()
            .expect("last_screenshot poisoned") = Some(png);

        Ok(true)
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (&app, &chrome, id, mode, rect, &state);
        Err(WisError::InvalidInput {
            message: "page capture is only available on macOS".to_string(),
        })
    }
}

/// Save the most recent screenshot (already on the clipboard) to a file chosen
/// via the native save panel. Backs the toast's "Save" button; `None` when
/// nothing has been captured yet or the panel is cancelled.
#[tauri::command]
pub async fn save_last_screenshot(
    app: AppHandle,
    state: State<'_, AppState>,
) -> WisResult<Option<String>> {
    let png = state
        .last_screenshot
        .lock()
        .expect("last_screenshot poisoned")
        .clone();
    let Some(png) = png else {
        return Ok(None);
    };

    #[cfg(target_os = "macos")]
    {
        let Some(path) = crate::panels::run_save_panel(&app, &screenshot_filename())? else {
            return Ok(None);
        };
        tokio::fs::write(&path, &png)
            .await
            .map_err(|err| WisError::InvalidInput {
                message: format!("writing {}: {err}", path.display()),
            })?;

        Ok(Some(path.to_string_lossy().into_owned()))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (&app, &png);
        Err(WisError::InvalidInput {
            message: "saving is only available on macOS".to_string(),
        })
    }
}

/// Suggested file name, e.g. `Screenshot 2026-06-23 at 14.30.05.png`.
fn screenshot_filename() -> String {
    let stamp = chrono::Local::now().format("%Y-%m-%d at %H.%M.%S");
    format!(
        "{}.png",
        wis_core::sanitize_filename(&format!("Screenshot {stamp}"))
    )
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;

    use block2::RcBlock;
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{class, msg_send};
    use std::ffi::{c_void, CString};

    /// `NSBitmapImageRep` PNG file-type tag.
    const NS_BITMAP_FILE_TYPE_PNG: usize = 4;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct NSPoint {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct NSSize {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct NSRect {
        origin: NSPoint,
        size: NSSize,
    }

    unsafe impl objc2::Encode for NSPoint {
        const ENCODING: objc2::Encoding = objc2::Encoding::Struct(
            "CGPoint",
            &[
                <f64 as objc2::Encode>::ENCODING,
                <f64 as objc2::Encode>::ENCODING,
            ],
        );
    }
    unsafe impl objc2::Encode for NSSize {
        const ENCODING: objc2::Encoding = objc2::Encoding::Struct(
            "CGSize",
            &[
                <f64 as objc2::Encode>::ENCODING,
                <f64 as objc2::Encode>::ENCODING,
            ],
        );
    }
    unsafe impl objc2::Encode for NSRect {
        const ENCODING: objc2::Encoding = objc2::Encoding::Struct(
            "CGRect",
            &[
                <NSPoint as objc2::Encode>::ENCODING,
                <NSSize as objc2::Encode>::ENCODING,
            ],
        );
    }

    /// Snapshot the tab to PNG bytes. `rect` is `(x, y, width, height)` in the
    /// webview's view coordinates, or `None` for the visible viewport.
    pub(super) async fn snapshot<R: tauri::Runtime>(
        webview: &tauri::webview::Webview<R>,
        rect: Option<(f64, f64, f64, f64)>,
    ) -> WisResult<Vec<u8>> {
        let (tx, rx) = mpsc::channel::<Option<Vec<u8>>>();
        webview
            .with_webview(move |w| unsafe {
                let wk: *mut AnyObject = w.inner() as *mut AnyObject;
                if wk.is_null() {
                    let _ = tx.send(None);
                    return;
                }

                let config: *mut AnyObject = match rect {
                    Some((x, y, width, height)) => {
                        let cfg: *mut AnyObject = msg_send![class!(WKSnapshotConfiguration), new];
                        let r = NSRect {
                            origin: NSPoint { x, y },
                            size: NSSize { width, height },
                        };
                        let _: () = msg_send![cfg, setRect: r];
                        let _: () = msg_send![cfg, setAfterScreenUpdates: Bool::YES];
                        cfg
                    }
                    None => std::ptr::null_mut(),
                };

                let block = RcBlock::new(move |image: *mut AnyObject, _err: *mut AnyObject| {
                    let _ = tx.send(image_to_png(image));
                });
                let _: () = msg_send![wk, takeSnapshotWithConfiguration: config, completionHandler: &*block];
                if !config.is_null() {
                    let _: () = msg_send![config, release];
                }
            })
            .map_err(|err| WisError::InvalidInput {
                message: format!("with_webview failed: {err}"),
            })?;

        recv_blocking(rx, Duration::from_secs(30))
            .await
            .flatten()
            .ok_or_else(|| WisError::InvalidInput {
                message: "the page could not be captured".to_string(),
            })
    }

    /// Capture the visible viewport or, when `full_page`, the whole scrollable
    /// document (height clamped to bound memory).
    pub(super) async fn capture<R: tauri::Runtime>(
        webview: &tauri::webview::Webview<R>,
        full_page: bool,
    ) -> WisResult<Vec<u8>> {
        let rect = if full_page {
            content_size(webview)
                .await
                .map(|(w, h)| (0.0, 0.0, w, h.min(MAX_CAPTURE_HEIGHT)))
        } else {
            None
        };

        snapshot(webview, rect).await
    }

    /// A captured webview layer: its PNG, its frame within the window's content
    /// view (points, in that view's own coordinate space), and the corner radius
    /// of its layer (points; `0.0` for a square layer like the chrome). The
    /// radius rounds the layer in the composite so it matches the on-screen
    /// webview, which `takeSnapshot` flattens to a rectangle.
    type Shot = (Vec<u8>, (f64, f64, f64, f64), f64);

    /// Content-view size + backing scale + whether the content view is flipped
    /// (top-left origin). Read off the chrome webview's window.
    type WinMetrics = (f64, f64, f64, bool);

    /// Capture the whole browser window **as it appears** by compositing the
    /// chrome webview (toolbar/sidebar/tab strip) with the live tab webviews
    /// stacked above it, all via `takeSnapshot` (no Screen Recording
    /// permission). `chrome` is the calling window's own webview.
    pub(super) async fn capture_window<R: tauri::Runtime>(
        app: &tauri::AppHandle<R>,
        chrome: &tauri::webview::Webview<R>,
        crop: Option<(f64, f64, f64, f64)>,
    ) -> WisResult<Vec<u8>> {
        // Base layer: the chrome, which also reports the window metrics.
        let (chrome_shot, metrics) = snapshot_layer(chrome, false)
            .await
            .ok_or_else(|| WisError::InvalidInput {
                message: "could not snapshot the window chrome".to_string(),
            })?;
        let (cw, ch, scale, flipped) = metrics;
        log::info!(
            "capture_window: chrome {cw}x{ch} @{scale}x flipped={flipped} crop={crop:?} frame={:?}",
            chrome_shot.1
        );

        let mut shots: Vec<Shot> = vec![chrome_shot];

        // Upper layers: every visible tab webview of this same window, in
        // arbitrary order (panes never overlap, so z-order among them is moot).
        let win_label = chrome.window().label().to_string();
        let tabs: Vec<_> = app
            .webviews()
            .iter()
            .filter(|(label, wv)| {
                label.starts_with("tab-") && wv.window().label() == win_label
            })
            .map(|(_, wv)| wv.clone())
            .collect();
        for tab in tabs {
            if let Some((shot, _)) = snapshot_layer(&tab, true).await {
                log::info!("capture_window: tab layer frame={:?} radius={}", shot.1, shot.2);
                shots.push(shot);
            }
        }

        composite_shots(app, shots, cw, ch, scale, flipped, crop)
            .await
            .ok_or_else(|| WisError::InvalidInput {
                message: "compositing the window capture failed".to_string(),
            })
    }

    /// Snapshot one webview for window compositing, reporting its layer [`Shot`]
    /// and the window metrics. `skip_hidden` returns `None` for an off-screen
    /// (hidden, or window-less) webview so inactive tabs are not composited.
    async fn snapshot_layer<R: tauri::Runtime>(
        webview: &tauri::webview::Webview<R>,
        skip_hidden: bool,
    ) -> Option<(Shot, WinMetrics)> {
        let (tx, rx) = mpsc::channel::<Option<(Shot, WinMetrics)>>();
        webview
            .with_webview(move |w| unsafe {
                let wk: *mut AnyObject = w.inner() as *mut AnyObject;
                if wk.is_null() {
                    let _ = tx.send(None);
                    return;
                }

                if skip_hidden {
                    let hidden: bool = msg_send![wk, isHidden];
                    let window: *mut AnyObject = msg_send![wk, window];
                    if hidden || window.is_null() {
                        let _ = tx.send(None);
                        return;
                    }
                }

                let frame = view_frame_in_content(wk);
                let radius = layer_corner_radius(wk);
                let metrics = win_metrics(wk);
                if frame.2 <= 0.0 || frame.3 <= 0.0 {
                    let _ = tx.send(None);
                    return;
                }

                let block = RcBlock::new(move |image: *mut AnyObject, _err: *mut AnyObject| {
                    let _ = tx.send(image_to_png(image).map(|png| ((png, frame, radius), metrics)));
                });
                let null_cfg: *mut AnyObject = std::ptr::null_mut();
                let _: () =
                    msg_send![wk, takeSnapshotWithConfiguration: null_cfg, completionHandler: &*block];
            })
            .ok()?;

        recv_blocking(rx, Duration::from_secs(30)).await.flatten()
    }

    /// The corner radius of a view's backing layer (points; `0.0` when the view
    /// is not layer-backed or has square corners). Tab webviews carry the
    /// design's radius via `set_webview_corner_radius`.
    unsafe fn layer_corner_radius(wk: *mut AnyObject) -> f64 {
        let layer: *mut AnyObject = msg_send![wk, layer];
        if layer.is_null() {
            return 0.0;
        }

        let r: f64 = msg_send![layer, cornerRadius];
        r.max(0.0)
    }

    /// A view's frame expressed in its window's content-view coordinate space.
    unsafe fn view_frame_in_content(wk: *mut AnyObject) -> (f64, f64, f64, f64) {
        let window: *mut AnyObject = msg_send![wk, window];
        if window.is_null() {
            return (0.0, 0.0, 0.0, 0.0);
        }

        let content: *mut AnyObject = msg_send![window, contentView];
        if content.is_null() {
            return (0.0, 0.0, 0.0, 0.0);
        }

        let bounds: NSRect = msg_send![wk, bounds];
        let r: NSRect = msg_send![content, convertRect: bounds, fromView: wk];

        (r.origin.x, r.origin.y, r.size.width, r.size.height)
    }

    /// Content-view size (points), backing scale, and flipped-ness.
    unsafe fn win_metrics(wk: *mut AnyObject) -> WinMetrics {
        let window: *mut AnyObject = msg_send![wk, window];
        if window.is_null() {
            return (0.0, 0.0, 1.0, false);
        }

        let content: *mut AnyObject = msg_send![window, contentView];
        if content.is_null() {
            return (0.0, 0.0, 1.0, false);
        }

        let b: NSRect = msg_send![content, bounds];
        let scale: f64 = msg_send![window, backingScaleFactor];
        let flipped: bool = msg_send![content, isFlipped];

        (b.size.width, b.size.height, scale, flipped)
    }

    /// Composite the layered PNGs into one window-sized PNG on the main thread.
    async fn composite_shots<R: tauri::Runtime>(
        app: &tauri::AppHandle<R>,
        shots: Vec<Shot>,
        cw: f64,
        ch: f64,
        scale: f64,
        flipped: bool,
        crop: Option<(f64, f64, f64, f64)>,
    ) -> Option<Vec<u8>> {
        let (tx, rx) = mpsc::channel::<Option<Vec<u8>>>();
        app.run_on_main_thread(move || {
            let _ = tx.send(unsafe { composite(&shots, cw, ch, scale, flipped, crop) });
        })
        .ok()?;

        recv_blocking(rx, Duration::from_secs(10)).await.flatten()
    }

    /// Draw each layer's PNG into an RGBA bitmap (pixel units) and encode it to
    /// PNG. Frames are converted from the content view's origin (top-left when
    /// `flipped`) to the bitmap's bottom-left origin. `crop` (a rect in top-left
    /// content points) restricts the output to that sub-region; `None` is the
    /// whole window.
    unsafe fn composite(
        shots: &[Shot],
        cw: f64,
        ch: f64,
        scale: f64,
        flipped: bool,
        crop: Option<(f64, f64, f64, f64)>,
    ) -> Option<Vec<u8>> {
        // Output region + a bottom-left origin offset into the full window. A
        // top-left crop `(rx, ry)` becomes the bottom-left offset `(rx, ch-ry-rh)`.
        let (off_x, off_y, out_w, out_h) = match crop {
            Some((rx, ry, rw, rh)) if rw > 0.0 && rh > 0.0 => (rx, ch - ry - rh, rw, rh),
            _ => (0.0, 0.0, cw, ch),
        };
        let pw = (out_w * scale).round() as isize;
        let ph = (out_h * scale).round() as isize;
        if pw <= 0 || ph <= 0 {
            return None;
        }

        let color_space = nsstring("NSDeviceRGBColorSpace")?;
        let rep: *mut AnyObject = msg_send![class!(NSBitmapImageRep), alloc];
        let rep: *mut AnyObject = msg_send![
            rep,
            initWithBitmapDataPlanes: std::ptr::null_mut::<*mut u8>(),
            pixelsWide: pw,
            pixelsHigh: ph,
            bitsPerSample: 8isize,
            samplesPerPixel: 4isize,
            hasAlpha: Bool::YES,
            isPlanar: Bool::NO,
            colorSpaceName: color_space,
            bytesPerRow: 0isize,
            bitsPerPixel: 0isize,
        ];
        if rep.is_null() {
            return None;
        }

        let ctx: *mut AnyObject =
            msg_send![class!(NSGraphicsContext), graphicsContextWithBitmapImageRep: rep];
        if ctx.is_null() {
            let _: () = msg_send![rep, release];
            return None;
        }

        let _: () = msg_send![class!(NSGraphicsContext), saveGraphicsState];
        let _: () = msg_send![class!(NSGraphicsContext), setCurrentContext: ctx];

        for (png, (fx, fy, fw, fh), radius) in shots {
            if *fw <= 0.0 || *fh <= 0.0 {
                continue;
            }

            let data: *mut AnyObject = msg_send![
                class!(NSData),
                dataWithBytes: png.as_ptr() as *const c_void,
                length: png.len(),
            ];
            if data.is_null() {
                continue;
            }

            let img: *mut AnyObject = msg_send![class!(NSImage), alloc];
            let img: *mut AnyObject = msg_send![img, initWithData: data];
            if img.is_null() {
                continue;
            }

            // The bitmap context has a bottom-left origin; convert a top-left
            // content-view y into it when the content view is flipped, then
            // shift by the crop offset so the cropped region maps to (0, 0).
            let draw_y = if flipped { ch - fy - fh } else { *fy };
            let dest = NSRect {
                origin: NSPoint {
                    x: (fx - off_x) * scale,
                    y: (draw_y - off_y) * scale,
                },
                size: NSSize {
                    width: fw * scale,
                    height: fh * scale,
                },
            };
            let zero = NSRect {
                origin: NSPoint { x: 0.0, y: 0.0 },
                size: NSSize {
                    width: 0.0,
                    height: 0.0,
                },
            };
            // Round the layer to match the on-screen webview, whose layer is
            // clipped to `radius`.
            if *radius > 0.0 {
                let r = radius * scale;
                let path: *mut AnyObject = msg_send![
                    class!(NSBezierPath),
                    bezierPathWithRoundedRect: dest,
                    xRadius: r,
                    yRadius: r,
                ];
                let _: () = msg_send![class!(NSGraphicsContext), saveGraphicsState];
                let _: () = msg_send![path, addClip];
                // NSCompositingOperationSourceOver = 2.
                let _: () = msg_send![img, drawInRect: dest, fromRect: zero, operation: 2usize, fraction: 1.0f64];
                let _: () = msg_send![class!(NSGraphicsContext), restoreGraphicsState];
            } else {
                // NSCompositingOperationSourceOver = 2.
                let _: () = msg_send![img, drawInRect: dest, fromRect: zero, operation: 2usize, fraction: 1.0f64];
            }
            let _: () = msg_send![img, release];
        }

        let _: () = msg_send![class!(NSGraphicsContext), restoreGraphicsState];

        let props: *mut AnyObject = msg_send![class!(NSDictionary), dictionary];
        let png_out: *mut AnyObject =
            msg_send![rep, representationUsingType: NS_BITMAP_FILE_TYPE_PNG, properties: props];
        let result = if png_out.is_null() {
            None
        } else {
            let len: usize = msg_send![png_out, length];
            let bytes: *const c_void = msg_send![png_out, bytes];
            if bytes.is_null() || len == 0 {
                None
            } else {
                Some(std::slice::from_raw_parts(bytes as *const u8, len).to_vec())
            }
        };
        let _: () = msg_send![rep, release];

        result
    }

    /// Read the document's scroll size for a full-page rect; `None` falls back
    /// to a visible-viewport capture.
    async fn content_size<R: tauri::Runtime>(
        webview: &tauri::webview::Webview<R>,
    ) -> Option<(f64, f64)> {
        let (tx, rx) = mpsc::channel::<Option<(f64, f64)>>();
        let js = "[document.documentElement.scrollWidth, document.documentElement.scrollHeight]";
        webview
            .with_webview(move |w| unsafe {
                let wk: *mut AnyObject = w.inner() as *mut AnyObject;
                if wk.is_null() {
                    let _ = tx.send(None);
                    return;
                }

                let Some(js_ns) = nsstring(js) else {
                    let _ = tx.send(None);
                    return;
                };

                let block = RcBlock::new(move |result: *mut AnyObject, _err: *mut AnyObject| {
                    let _ = tx.send(parse_size(result));
                });
                let _: () = msg_send![wk, evaluateJavaScript: js_ns, completionHandler: &*block];
            })
            .ok()?;

        recv_blocking(rx, Duration::from_secs(5)).await.flatten()
    }

    /// Copy PNG bytes to the general pasteboard as an image. Runs on the main
    /// thread (AppKit), bridged back over a channel.
    pub(super) async fn copy_image_to_pasteboard(app: &AppHandle, png: Vec<u8>) -> bool {
        let (tx, rx) = mpsc::channel::<bool>();
        if app
            .run_on_main_thread(move || {
                let _ = tx.send(unsafe { write_image_pasteboard(&png) });
            })
            .is_err()
        {
            return false;
        }

        recv_blocking(rx, Duration::from_secs(3)).await.unwrap_or(false)
    }

    /// Await a channel value off the async runtime so the worker isn't blocked.
    async fn recv_blocking<T: Send + 'static>(
        rx: mpsc::Receiver<T>,
        timeout: Duration,
    ) -> Option<T> {
        tokio::task::spawn_blocking(move || rx.recv_timeout(timeout).ok())
            .await
            .ok()
            .flatten()
    }

    unsafe fn nsstring(s: &str) -> Option<*mut AnyObject> {
        let c = CString::new(s).ok()?;
        let ns: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()];
        (!ns.is_null()).then_some(ns)
    }

    /// Parse `[width, height]` (an `NSArray` of two `NSNumber`s) into pixels.
    unsafe fn parse_size(result: *mut AnyObject) -> Option<(f64, f64)> {
        if result.is_null() {
            return None;
        }

        let is_array: bool = msg_send![result, isKindOfClass: class!(NSArray)];
        if !is_array {
            return None;
        }

        let count: usize = msg_send![result, count];
        if count < 2 {
            return None;
        }

        let w_num: *mut AnyObject = msg_send![result, objectAtIndex: 0usize];
        let h_num: *mut AnyObject = msg_send![result, objectAtIndex: 1usize];
        if w_num.is_null() || h_num.is_null() {
            return None;
        }

        let width: f64 = msg_send![w_num, doubleValue];
        let height: f64 = msg_send![h_num, doubleValue];
        if width <= 0.0 || height <= 0.0 {
            return None;
        }

        Some((width, height))
    }

    /// Encode an `NSImage` to PNG bytes via `NSBitmapImageRep`.
    unsafe fn image_to_png(image: *mut AnyObject) -> Option<Vec<u8>> {
        if image.is_null() {
            return None;
        }

        let tiff: *mut AnyObject = msg_send![image, TIFFRepresentation];
        if tiff.is_null() {
            return None;
        }

        let rep: *mut AnyObject = msg_send![class!(NSBitmapImageRep), imageRepWithData: tiff];
        if rep.is_null() {
            return None;
        }

        let props: *mut AnyObject = msg_send![class!(NSDictionary), dictionary];
        let png: *mut AnyObject =
            msg_send![rep, representationUsingType: NS_BITMAP_FILE_TYPE_PNG, properties: props];
        if png.is_null() {
            return None;
        }

        let len: usize = msg_send![png, length];
        // `-[NSData bytes]` returns `const void *` (`^v`); declaring `*const u8`
        // trips objc2's return-type check and aborts.
        let bytes: *const c_void = msg_send![png, bytes];
        if bytes.is_null() || len == 0 {
            return None;
        }

        Some(std::slice::from_raw_parts(bytes as *const u8, len).to_vec())
    }

    unsafe fn write_image_pasteboard(png: &[u8]) -> bool {
        let data: *mut AnyObject = msg_send![
            class!(NSData),
            dataWithBytes: png.as_ptr() as *const c_void,
            length: png.len(),
        ];
        if data.is_null() {
            return false;
        }

        let image: *mut AnyObject = msg_send![class!(NSImage), alloc];
        let image: *mut AnyObject = msg_send![image, initWithData: data];
        if image.is_null() {
            return false;
        }

        let pb: *mut AnyObject = msg_send![class!(NSPasteboard), generalPasteboard];
        if pb.is_null() {
            let _: () = msg_send![image, release];
            return false;
        }

        let _: isize = msg_send![pb, clearContents];
        let arr: *mut AnyObject = msg_send![class!(NSArray), arrayWithObject: image];
        let ok: bool = msg_send![pb, writeObjects: arr];
        let _: () = msg_send![image, release];

        ok
    }
}

#[cfg(test)]
mod tests {
    use super::screenshot_filename;

    #[test]
    fn filename_is_png_and_sanitized() {
        let name = screenshot_filename();
        assert!(name.ends_with(".png"));
        assert!(name.starts_with("Screenshot "));
        // No path separators or other unsafe characters survive sanitization.
        assert!(!name.contains('/'));
    }
}
