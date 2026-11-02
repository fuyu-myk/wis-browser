//! Element fullscreen for tabs on Windows.
//!
//! WebView2 keeps a fullscreen element inside the webview's own bounds, and
//! Tauri only makes the window fullscreen along with the page, so the video
//! stayed inside its pane. Here the tab fills the window while its page is
//! fullscreen, and the window is put back when the tab is switched away or
//! closes. macOS and Linux leave this to the engine.

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Bounds {
    /// Wholly outside a window of `size`, as the chrome parks a hidden tab.
    pub fn outside(&self, (width, height): (f64, f64)) -> bool {
        self.x >= width
            || self.y >= height
            || self.x + self.width <= 0.0
            || self.y + self.height <= 0.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Held {
    pub tab: String,
    pub pane: Bounds,
}

#[derive(Debug, PartialEq)]
pub(crate) enum Hold {
    Free,
    Fill,
    Release,
}

/// The tab holding each window, by window label.
#[derive(Debug, Default)]
pub(crate) struct Fullscreens(HashMap<String, Held>);

impl Fullscreens {
    /// Hand `window` to `tab`, dropping windows no longer open. False when a
    /// tab already holds it.
    pub fn enter(
        &mut self,
        live_windows: &[String],
        window: &str,
        tab: &str,
        pane: Bounds,
    ) -> bool {
        self.0.retain(|label, _| live_windows.contains(label));
        if self.0.contains_key(window) {
            return false;
        }

        let held = Held {
            tab: tab.to_string(),
            pane,
        };
        self.0.insert(window.to_string(), held);
        true
    }

    pub fn holds(&self, window: &str, tab: &str) -> bool {
        self.0.get(window).is_some_and(|held| held.tab == tab)
    }

    pub fn leave(&mut self, window: &str, tab: &str) -> Option<Held> {
        if !self.holds(window, tab) {
            return None;
        }

        self.0.remove(window)
    }

    pub fn forget(&mut self, tab: &str) -> Option<(String, Held)> {
        let window = self.0.iter().find(|(_, held)| held.tab == tab)?.0.clone();
        self.0.remove_entry(&window)
    }

    /// Bounds the chrome pushed for `tab`. A holding tab fills the window and
    /// keeps the pane it entered from; bounds outside the window end the hold.
    pub fn hold(&mut self, window: &str, tab: &str, pushed: Bounds, size: (f64, f64)) -> Hold {
        if !self.holds(window, tab) {
            return Hold::Free;
        }
        if !pushed.outside(size) {
            return Hold::Fill;
        }

        self.0.remove(window);
        Hold::Release
    }
}

#[cfg(windows)]
fn held() -> std::sync::MutexGuard<'static, Fullscreens> {
    static HELD: std::sync::OnceLock<std::sync::Mutex<Fullscreens>> = std::sync::OnceLock::new();
    HELD.get_or_init(Default::default)
        .lock()
        .expect("element fullscreen poisoned")
}

/// Follow the tab's `ContainsFullScreenElement`. Called per tab webview.
#[cfg(windows)]
pub fn install(webview: &tauri::Webview) {
    use tauri::Manager;
    use webview2_com::ContainsFullScreenElementChangedEventHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2;

    let app = webview.app_handle().clone();
    let label = webview.label().to_string();
    crate::platform::win_webview2::with_core_webview2(webview, move |core| {
        let handler = ContainsFullScreenElementChangedEventHandler::create(Box::new(
            move |sender: Option<ICoreWebView2>,
                  _args: Option<windows::core::IUnknown>|
                  -> windows::core::Result<()> {
                let Some(core) = sender else {
                    return Ok(());
                };
                let mut full = windows::core::BOOL(0);
                // SAFETY: UI thread (with_core_webview2); the out-param is a stack local.
                unsafe { core.ContainsFullScreenElement(&mut full)? };
                if full.as_bool() {
                    enter(&app, &label);
                } else {
                    leave(&app, &label);
                }

                Ok(())
            },
        ));
        let mut token = 0_i64;

        // SAFETY: runs on the UI thread via with_core_webview2.
        if let Err(err) = unsafe { core.add_ContainsFullScreenElementChanged(&handler, &mut token) }
        {
            log::warn!("element_fullscreen: add_ContainsFullScreenElementChanged failed: {err}");
        }
    });
}

/// Bounds the chrome pushed for a tab. True when the tab holds its window and
/// fills it instead.
#[cfg(windows)]
pub fn hold_bounds(webview: &tauri::Webview, x: f64, y: f64, width: f64, height: f64) -> bool {
    use tauri::Manager;

    let window = webview.window();
    if !held().holds(window.label(), webview.label()) {
        return false;
    }
    let Some(size) = logical_size(&window) else {
        return false;
    };

    let pushed = Bounds {
        x,
        y,
        width,
        height,
    };
    let hold = held().hold(window.label(), webview.label(), pushed, size);
    match hold {
        Hold::Free => false,
        Hold::Fill => {
            crate::platform::win_webview2::clear_corner_region(webview);
            place(webview, 0.0, 0.0, size);
            true
        }
        Hold::Release => {
            release(webview.app_handle(), &window);
            exit_page_fullscreen(webview);
            false
        }
    }
}

/// A closed tab gives its window back.
#[cfg(windows)]
pub fn forget(app: &tauri::AppHandle, label: &str) {
    use tauri::Manager;

    let left = held().forget(label);
    let Some((window, _)) = left else {
        return;
    };
    if let Some(window) = app.get_window(&window) {
        release(app, &window);
    }
}

#[cfg(windows)]
fn enter(app: &tauri::AppHandle, label: &str) {
    use tauri::Manager;

    let Some(tab) = app.get_webview(label) else {
        return;
    };
    let window = tab.window();
    let (Ok(scale), Ok(position), Ok(size)) = (window.scale_factor(), tab.position(), tab.size())
    else {
        return;
    };
    let position = position.to_logical::<f64>(scale);
    let size = size.to_logical::<f64>(scale);
    let pane = Bounds {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
    };

    if logical_size(&window).is_none_or(|size| pane.outside(size)) {
        exit_page_fullscreen(&tab);
        return;
    }
    let live: Vec<String> = app.windows().into_keys().collect();
    if !held().enter(&live, window.label(), label, pane) {
        return;
    }

    if let Err(err) = window.set_fullscreen(true) {
        log::warn!("element_fullscreen: window fullscreen failed: {err}");
    }
    crate::platform::win_webview2::clear_corner_region(&tab);
    if let Some(size) = logical_size(&window) {
        place(&tab, 0.0, 0.0, size);
    }
    crate::platform::win_webview2::promote_to_front(&tab);
}

#[cfg(windows)]
fn leave(app: &tauri::AppHandle, label: &str) {
    use tauri::Manager;

    let Some(tab) = app.get_webview(label) else {
        return;
    };
    let window = tab.window();
    let left = held().leave(window.label(), label);
    let Some(left) = left else {
        return;
    };

    release(app, &window);
    let pane = left.pane;
    place(&tab, pane.x, pane.y, (pane.width, pane.height));
    crate::platform::win_webview2::reapply_corner_radius(&tab, (pane.width, pane.height));
}

/// Undo what holding the window changed, restoring tabs < panels < overlays.
#[cfg(windows)]
fn release(app: &tauri::AppHandle, window: &tauri::Window) {
    if let Err(err) = window.set_fullscreen(false) {
        log::warn!("element_fullscreen: window restore failed: {err}");
    }
    crate::chrome::widget_panel::promote_panels_to_front(app, window.label());
    crate::chrome::overlay::promote_overlays_to_front(app, window.label());
}

#[cfg(windows)]
fn exit_page_fullscreen(tab: &tauri::Webview) {
    if let Err(err) =
        tab.eval("if (document.fullscreenElement) document.exitFullscreen().catch(function () {});")
    {
        log::warn!("element_fullscreen: exit page fullscreen failed: {err}");
    }
}

#[cfg(windows)]
fn logical_size(window: &tauri::Window) -> Option<(f64, f64)> {
    let (Ok(scale), Ok(size)) = (window.scale_factor(), window.inner_size()) else {
        return None;
    };
    let size = size.to_logical::<f64>(scale);
    Some((size.width, size.height))
}

#[cfg(windows)]
fn place(tab: &tauri::Webview, x: f64, y: f64, (width, height): (f64, f64)) {
    if let Err(err) = tab.set_position(tauri::LogicalPosition::new(x, y)) {
        log::warn!("element_fullscreen: move tab failed: {err}");
    }
    if let Err(err) = tab.set_size(tauri::LogicalSize::new(width, height)) {
        log::warn!("element_fullscreen: size tab failed: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANE: Bounds = Bounds {
        x: 240.0,
        y: 48.0,
        width: 1000.0,
        height: 700.0,
    };
    const WINDOW: (f64, f64) = (1920.0, 1080.0);

    fn live() -> Vec<String> {
        vec!["main".to_string(), "window-2".to_string()]
    }

    #[test]
    fn a_tab_holds_its_window_until_it_leaves() {
        let mut held = Fullscreens::default();
        assert!(held.enter(&live(), "main", "tab-1", PANE));

        assert_eq!(held.leave("main", "tab-2"), None);
        assert_eq!(
            held.leave("main", "tab-1"),
            Some(Held {
                tab: "tab-1".into(),
                pane: PANE,
            })
        );
        assert_eq!(held.leave("main", "tab-1"), None);
    }

    #[test]
    fn a_held_window_keeps_its_first_tab_and_pane() {
        let mut held = Fullscreens::default();
        assert!(held.enter(&live(), "main", "tab-1", PANE));

        let full = Bounds {
            x: 0.0,
            y: 0.0,
            width: WINDOW.0,
            height: WINDOW.1,
        };
        assert!(!held.enter(&live(), "main", "tab-1", full));
        assert!(!held.enter(&live(), "main", "tab-2", full));
        assert!(held.enter(&live(), "window-2", "tab-3", PANE));

        let first = held.leave("main", "tab-1").expect("tab-1 holds main");
        assert_eq!(first.pane, PANE);
    }

    #[test]
    fn a_closed_window_is_dropped_on_the_next_entry() {
        let mut held = Fullscreens::default();
        held.enter(&live(), "window-2", "tab-3", PANE);
        held.enter(&["main".to_string()], "main", "tab-1", PANE);

        assert_eq!(held.leave("window-2", "tab-3"), None);
    }

    #[test]
    fn a_held_tab_fills_its_window_and_returns_to_the_pane_it_left() {
        let mut held = Fullscreens::default();
        held.enter(&live(), "main", "tab-1", PANE);

        let wider = Bounds {
            width: 1600.0,
            ..PANE
        };
        assert_eq!(held.hold("main", "tab-1", wider, WINDOW), Hold::Fill);
        assert_eq!(held.hold("main", "tab-2", wider, WINDOW), Hold::Free);
        assert_eq!(held.hold("window-2", "tab-1", wider, WINDOW), Hold::Free);

        assert_eq!(held.leave("main", "tab-1").map(|h| h.pane), Some(PANE));
    }

    #[test]
    fn a_tab_parked_outside_its_window_lets_go() {
        for outside in outside_bounds() {
            let mut held = Fullscreens::default();
            held.enter(&live(), "main", "tab-1", PANE);

            assert_eq!(
                held.hold("main", "tab-1", outside, WINDOW),
                Hold::Release,
                "{outside:?}"
            );
            assert_eq!(held.leave("main", "tab-1"), None);
        }
    }

    #[test]
    fn only_bounds_wholly_outside_the_window_are_outside_it() {
        for outside in outside_bounds() {
            assert!(outside.outside(WINDOW), "{outside:?}");
        }

        let edge = Bounds {
            x: -999.0,
            y: 1079.0,
            ..PANE
        };
        assert!(!PANE.outside(WINDOW));
        assert!(!edge.outside(WINDOW));
    }

    #[test]
    fn a_closed_tab_is_forgotten_with_its_window() {
        let mut held = Fullscreens::default();
        held.enter(&live(), "window-2", "tab-3", PANE);

        assert_eq!(held.forget("tab-1"), None);
        let (window, left) = held.forget("tab-3").expect("tab-3 holds window-2");
        assert_eq!((window.as_str(), left.pane), ("window-2", PANE));
        assert_eq!(held.forget("tab-3"), None);
    }

    fn outside_bounds() -> [Bounds; 4] {
        let parked = Bounds {
            x: 100_000.0,
            y: 100_000.0,
            width: 800.0,
            height: 600.0,
        };
        let above = Bounds { y: -700.0, ..PANE };
        let left = Bounds { x: -1000.0, ..PANE };
        let below = Bounds { y: 1080.0, ..PANE };
        [parked, above, left, below]
    }
}
