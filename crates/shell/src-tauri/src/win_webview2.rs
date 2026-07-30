//! Shared access to the WebView2 handles behind a Tauri webview on Windows.
//!
//! Tauri's `PlatformWebview` exposes the WebView2 `controller`/`environment`
//! only on Windows, and `with_webview` runs its closure on the UI thread, which
//! is where COM calls must happen. These helpers centralize the
//! controller -> CoreWebView2 plumbing the per-feature Windows ports share,
//! so each feature only writes the one or two calls unique to it.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2Controller, COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC,
};
use windows::Win32::Foundation::HWND;

/// Run `f` with the webview's live `ICoreWebView2`, on the UI thread. A no-op
/// if the controller has no `CoreWebView2` yet.
pub fn with_core_webview2<R, F>(webview: &tauri::webview::Webview<R>, f: F)
where
    R: tauri::Runtime,
    F: FnOnce(&ICoreWebView2) + Send + 'static,
{
    let _ = webview.with_webview(move |w| {
        // SAFETY: dispatched to the UI thread by `with_webview`; `CoreWebView2`
        // is a plain getter on the controller and the handle never leaves here.
        if let Ok(core) = unsafe { w.controller().CoreWebView2() } {
            f(&core);
        }
    });
}

/// Run `f` with the webview's `ICoreWebView2Controller`, on the UI thread.
pub fn with_controller<R, F>(webview: &tauri::webview::Webview<R>, f: F)
where
    R: tauri::Runtime,
    F: FnOnce(&ICoreWebView2Controller) + Send + 'static,
{
    // SAFETY: dispatched to the UI thread by `with_webview`; the controller
    // handle stays inside the closure.
    let _ = webview.with_webview(move |w| f(&w.controller()));
}

/// Run `f` with the wry container HWND that hosts the webview's WebView2
/// controller, on the UI thread. The container is the controller's parent
/// window: one dedicated child HWND per webview, so window-level operations
/// (region, z-order) affect exactly this webview and everything it renders.
pub fn with_container_hwnd<R, F>(webview: &tauri::webview::Webview<R>, f: F)
where
    R: tauri::Runtime,
    F: FnOnce(HWND) + Send + 'static,
{
    with_controller(webview, move |controller| {
        let mut hwnd = HWND::default();
        // SAFETY: runs on the UI thread via `with_controller`; `ParentWindow`
        // is a plain getter.
        if unsafe { controller.ParentWindow(&mut hwnd) }.is_ok() && !hwnd.is_invalid() {
            f(hwnd);
        }
    });
}

/// Registered corner radius per webview label, so a bounds change can re-cut
/// the window region at the new size.
static CORNER_RADII: OnceLock<Mutex<HashMap<String, f64>>> = OnceLock::new();

fn corner_radii() -> &'static Mutex<HashMap<String, f64>> {
    CORNER_RADII.get_or_init(|| Mutex::new(HashMap::new()))
}

fn registered_radius(label: &str) -> Option<f64> {
    corner_radii()
        .lock()
        .expect("CORNER_RADII poisoned")
        .get(label)
        .copied()
}

/// Round the webview's corners by cutting a rounded-rect window region on its
/// container HWND, the Windows analogue of the macOS `layer.cornerRadius` +
/// `masksToBounds` pair. Clips everything the engine draws, scrollbar included.
/// Registers the radius so [`reapply_corner_radius`] can re-cut after resizes.
pub fn set_corner_radius<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>, radius: f64) {
    use tauri::Manager;

    // Tab webviews are closed frontend-side with no Rust hook, so prune any
    // registration whose webview no longer exists here (this runs once per new
    // webview): the map stays bounded to live webviews without an extra IPC.
    let live: std::collections::HashSet<String> =
        webview.app_handle().webviews().into_keys().collect();
    {
        let mut map = corner_radii().lock().expect("CORNER_RADII poisoned");
        map.retain(|label, _| live.contains(label));
        map.insert(webview.label().to_string(), radius);
    }

    apply_region(webview, radius, None);
}

/// Re-cut the rounded region for a webview whose bounds just changed. The new
/// logical size is passed in (wry applies the resize asynchronously, so the
/// current client rect may still be stale). No-op for webviews that never
/// registered a radius.
pub fn reapply_corner_radius<R: tauri::Runtime>(
    webview: &tauri::webview::Webview<R>,
    logical_size: (f64, f64),
) {
    if let Some(radius) = registered_radius(webview.label()) {
        apply_region(webview, radius, Some(logical_size));
    }
}

/// Drop a closed webview's radius registration.
pub fn forget_corner_radius(label: &str) {
    corner_radii()
        .lock()
        .expect("CORNER_RADII poisoned")
        .remove(label);
}

fn apply_region<R: tauri::Runtime>(
    webview: &tauri::webview::Webview<R>,
    radius: f64,
    logical_size: Option<(f64, f64)>,
) {
    let scale = webview
        .window()
        .scale_factor()
        .ok()
        .filter(|s| s.is_finite() && *s > 0.0)
        .unwrap_or(1.0);

    with_container_hwnd(webview, move |hwnd| {
        use windows::Win32::Foundation::RECT;
        use windows::Win32::Graphics::Gdi::{CreateRoundRectRgn, DeleteObject, SetWindowRgn};
        use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

        // SAFETY: runs on the UI thread; the HWND is live for the duration of
        // the closure and the region handle is either owned by the window
        // after SetWindowRgn succeeds or deleted here on failure.
        unsafe {
            let (width, height) = match logical_size {
                Some((w, h)) => ((w * scale).round() as i32, (h * scale).round() as i32),
                None => {
                    let mut rect = RECT::default();
                    if GetClientRect(hwnd, &mut rect).is_err() {
                        return;
                    }

                    (rect.right - rect.left, rect.bottom - rect.top)
                }
            };
            if width <= 0 || height <= 0 {
                return;
            }

            let diameter = (2.0 * radius * scale).round() as i32;
            let region = CreateRoundRectRgn(0, 0, width + 1, height + 1, diameter, diameter);
            if region.is_invalid() {
                return;
            }

            if SetWindowRgn(hwnd, Some(region), true) == 0 {
                let _ = DeleteObject(region.into());
            }
        }
    });
}

/// Raise the webview's container HWND to the top of its window's child stack.
/// The Windows analogue of the macOS `addSubview:` re-add used to keep the
/// tabs < widget panels < overlays order.
pub fn promote_to_front<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    with_container_hwnd(webview, |hwnd| {
        use windows::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        };
        // SAFETY: runs on the UI thread; z-order-only reposition.
        let _ = unsafe {
            SetWindowPos(
                hwnd,
                Some(HWND_TOP),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
        };
    });
}

/// [`promote_to_front`], but only when the container is currently hidden
/// (being revealed).
pub fn promote_to_front_when_hidden<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    with_container_hwnd(webview, |hwnd| {
        use windows::Win32::UI::WindowsAndMessaging::{
            IsWindowVisible, SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        };

        // SAFETY: runs on the UI thread; visibility read + z-order-only move.
        unsafe {
            if IsWindowVisible(hwnd).as_bool() {
                return;
            }

            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOP),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    });
}

/// Move Win32 keyboard focus into the webview's WebView2 controller.
pub fn move_focus<R: tauri::Runtime>(webview: &tauri::webview::Webview<R>) {
    with_controller(webview, |controller| {
        // SAFETY: runs on the UI thread via `with_controller`.
        let _ = unsafe { controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC) };
    });
}
