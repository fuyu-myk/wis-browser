//! Two-finger swipe back/forward with a custom edge indicator.
//!
//! The native `allowsBackForwardNavigationGestures` is all-or-nothing and gives
//! no progress to drive UI, so this replaces it with an injected page script:
//! it watches `wheel` overscroll at the page's true horizontal edge, draws a
//! themed half-circle indicator that peeks in from the side and *fills* at the
//! commit threshold, and on release past the threshold calls
//! `history.back()`/`forward()` (so the usual page-load hook keeps the URL bar
//! in sync). The indicator lives in a shadow root for style isolation and is
//! drawn in-page so it tracks the gesture with zero IPC latency.
//!
//! Wheel events carry no gesture phase in JS, so the shell supplies the two
//! signals the script cannot read itself:
//!
//! - **Finger lift** (macOS): an app-wide `NSEvent` scroll-wheel monitor
//!   ([`install_phase_monitor`]) forwards `began`/`ended` phase transitions to
//!   every tab webview. Commit fires exactly at lift, and the momentum tail
//!   after it never moves the indicator or re-engages a gesture. Off macOS the
//!   script falls back to a wheel-quiet debounce.
//! - **History availability**: `canGoBack`/`canGoForward` are pushed per
//!   direction after each load and title change ([`push_nav_state`], both
//!   platforms), so the gesture only engages toward a direction that can
//!   actually navigate. `pushState` is tracked in-page (it adds a back entry
//!   and clears the forward list); anything else falls back to
//!   `history.length`.
//!
//! The scroll-vs-gesture call is made **once per wheel sequence** (finger down
//! to lift + momentum end): a sequence that starts vertical, over a
//! horizontally-scrollable element, or away from the page edge is a scroll for
//! its whole lifetime, so an inner carousel reaching its edge mid-swipe can
//! never turn into a navigation.
//!
//! The accent colour is baked per tab via a tiny `window.__wisSwipeAccent`
//! script (see `create_tab_webview`), built from the workspace's accent /
//! custom accent. Feel constants live at the top of the script for tuning.

/// Builder-level script (run at document start) implementing the gesture + UI.
const SWIPE_NAV_JS: &str = r#"(function () {
  if (window.__wisSwipeNav) return;
  window.__wisSwipeNav = true;

  var THRESHOLD = 110;     // px of edge overscroll to arm a commit
  var R = 26;              // indicator radius (px); bulge depth = R, height = 2R
  var M = 4;               // viewBox padding so the stroke/apex is never clipped flat
  var END_DELAY = 90;      // no-phase fallback: ms of wheel quiet = fingers lifted
  var SAFETY_DELAY = 1200; // native mode: wheel quiet after which a gesture is abandoned
  var MIN_DECIDE = 3;      // |dx|+|dy| below this is too small to classify a sequence

  function accent() { return window.__wisSwipeAccent || '#6b77f8'; }

  // Horizontal overscroll must never move the page.
  function lockOverscroll() {
    var de = document.documentElement;
    if (de && de.style.overscrollBehaviorX !== 'none') de.style.overscrollBehaviorX = 'none';
  }
  lockOverscroll();
  document.addEventListener('DOMContentLoaded', lockOverscroll);

  // Black/white arrow for the filled state, by the accent's relative luminance.
  function contrast(hex) {
    try {
      var h = String(hex).replace('#', '');
      if (h.length === 3) h = h[0] + h[0] + h[1] + h[1] + h[2] + h[2];
      function lin(c) { c /= 255; return c <= 0.03928 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); }
      var L = 0.2126 * lin(parseInt(h.slice(0, 2), 16))
            + 0.7152 * lin(parseInt(h.slice(2, 4), 16))
            + 0.0722 * lin(parseInt(h.slice(4, 6), 16));
      return L > 0.45 ? '#000000' : '#ffffff';
    } catch (e) { return '#ffffff'; }
  }

  function canScroll(el, sign) {
    while (el && el.nodeType === 1 && el !== document.documentElement) {
      var ov = getComputedStyle(el).overflowX;
      if ((ov === 'auto' || ov === 'scroll') && el.scrollWidth > el.clientWidth + 1) {
        if (sign < 0 && el.scrollLeft > 0) return true;
        if (sign > 0 && el.scrollLeft < el.scrollWidth - el.clientWidth - 1) return true;
      }
      el = el.parentNode;
    }
    var de = document.scrollingElement || document.documentElement;
    if (sign < 0 && de.scrollLeft > 0) return true;
    if (sign > 0 && de.scrollLeft < de.scrollWidth - de.clientWidth - 1) return true;
    return false;
  }

  var host, semi, arr, ind;
  function build(dir) {
    if (!host) {
      host = document.createElement('div');
      host.style.cssText = 'position:fixed;inset:0;z-index:2147483647;pointer-events:none;';
      var sh = host.attachShadow({ mode: 'open' });
      var st = document.createElement('style');
      st.textContent =
        '.ind{position:fixed;top:50%;transform:translateY(-50%) translateX(var(--tx,0));opacity:0;transition:opacity .12s ease;}'
        + 'svg{overflow:visible;shape-rendering:geometricPrecision;}'
        + '.semi{stroke-width:2;stroke-linecap:round;fill-opacity:0;transition:fill-opacity .14s ease;}'
        + '.arr{fill:none;stroke-width:2.6;stroke-linecap:round;stroke-linejoin:round;transition:stroke .14s ease;}';
      sh.appendChild(st);
      ind = document.createElement('div');
      ind.className = 'ind';
      sh.appendChild(ind);
      document.documentElement.appendChild(host);
    }
    var a = accent();
    var back = dir === 'back';
    var W = R + M, H = 2 * R + 2 * M, cy = R + M, hw = R * 0.32;
    var semiD = back
      ? 'M0 ' + M + ' A ' + R + ' ' + R + ' 0 0 1 0 ' + (2 * R + M)
      : 'M' + W + ' ' + M + ' A ' + R + ' ' + R + ' 0 0 0 ' + W + ' ' + (2 * R + M);
    var tipX = back ? R * 0.28 : W - R * 0.28;
    var baseX = back ? R * 0.56 : W - R * 0.56;
    var arrD = 'M' + baseX + ' ' + (cy - hw) + ' L' + tipX + ' ' + cy + ' L' + baseX + ' ' + (cy + hw);
    ind.innerHTML = '<svg width="' + W + '" height="' + H + '" viewBox="0 0 ' + W + ' ' + H + '">'
      + '<path class="semi" d="' + semiD + '" stroke="' + a + '" fill="' + a + '"/>'
      + '<path class="arr" d="' + arrD + '" stroke="' + a + '"/></svg>';
    ind.style.left = back ? '0' : 'auto';
    ind.style.right = back ? 'auto' : '0';
    ind.dataset.dir = dir;
    semi = ind.querySelector('.semi');
    arr = ind.querySelector('.arr');
  }

  function show(dir) { build(dir); ind.style.opacity = '1'; }
  function update(p, dir) {
    // Emerge from the edge as progress grows (hidden at 0, flush at 1).
    var tx = (dir === 'back' ? -1 : 1) * (R + M) * (1 - Math.min(p, 1));
    ind.style.setProperty('--tx', tx + 'px');
  }
  function setArmed(on) {
    if (!semi) return;
    semi.style.fillOpacity = on ? '1' : '0';
    arr.setAttribute('stroke', on ? contrast(accent()) : accent());
  }
  function hide() { if (ind) ind.style.opacity = '0'; }

  // Session-history availability, pushed by the shell after loads and title
  // changes.
  var navState = { back: null, fwd: null };
  window.__wisSwipeNavState = function (back, fwd) {
    navState.back = !!back;
    navState.fwd = !!fwd;
  };
  try {
    var origPush = history.pushState;
    history.pushState = function () {
      var r = origPush.apply(this, arguments);
      navState.back = true;
      navState.fwd = false;
      return r;
    };
  } catch (e) {}
  window.addEventListener('popstate', function () { navState.back = null; navState.fwd = null; });

  function canNavigate(dir) {
    var known = dir === 'back' ? navState.back : navState.fwd;
    if (known !== null) return known;
    return window.history.length > 1;
  }

  // One wheel "sequence" runs from finger-down (or the first wheel event) to
  // lift + momentum end, and is classified exactly once at its start.
  var seq = null;      // null | 'scroll' | 'gesture'
  var active = null;   // gesture state while seq === 'gesture'
  var fingersDown = false;
  var nativePhase = !!window.__wisSwipeHasPhase;
  var quietTimer = null;

  function armQuiet(ms, fn) { clearTimeout(quietTimer); quietTimer = setTimeout(fn, ms); }

  function finalize() {
    clearTimeout(quietTimer);
    seq = null;
    if (!active) return;
    var commit = active.armed, dir = active.dir;
    active = null;
    hide();
    if (commit) { if (dir === 'back') history.back(); else history.forward(); }
  }

  function cancel() {
    clearTimeout(quietTimer);
    seq = null;
    active = null;
    hide();
  }

  window.__wisSwipePhase = function (p) {
    nativePhase = true;
    if (p === 'began') {
      fingersDown = true;
      if (active) cancel();
      seq = null;
    } else if (p === 'ended') {
      fingersDown = false;
      finalize();
    }
  };

  window.addEventListener('wheel', function (e) {
    // Native mode: everything after finger lift is momentum; it neither moves
    // the indicator nor starts a new sequence.
    if (nativePhase && !fingersDown) return;

    if (seq === 'scroll') {
      if (!nativePhase) armQuiet(END_DELAY, function () { seq = null; });
      return;
    }

    if (seq === null) {
      if (Math.abs(e.deltaX) + Math.abs(e.deltaY) < MIN_DECIDE) return;
      var horizontal = Math.abs(e.deltaX) > Math.abs(e.deltaY);
      var dir = e.deltaX < 0 ? 'back' : 'forward';
      var sign = e.deltaX < 0 ? -1 : 1;
      if (!e.cancelable || !horizontal || canScroll(e.target, sign) || !canNavigate(dir)) {
        seq = 'scroll';
        if (!nativePhase) armQuiet(END_DELAY, function () { seq = null; });
        return;
      }
      seq = 'gesture';
      active = { dir: dir, acc: 0, armed: false };
      show(dir);
    }

    if (!active) return;

    if (!e.cancelable) {
      seq = 'scroll';
      active = null;
      hide();
      return;
    }

    active.acc += active.dir === 'back' ? -e.deltaX : e.deltaX;
    if (active.acc < 0) active.acc = 0;
    e.preventDefault();
    e.stopPropagation();

    update(active.acc / THRESHOLD, active.dir);
    var armed = active.acc >= THRESHOLD;
    if (armed !== active.armed) { active.armed = armed; setArmed(armed); }

    if (nativePhase) armQuiet(SAFETY_DELAY, cancel);
    else armQuiet(END_DELAY, finalize);
  }, { passive: false, capture: true });
})();"#;

/// The swipe-nav script, exposed so tab webviews bake it in as a builder-level
/// initialization script (runs at document start, before the first document).
pub fn page_script() -> &'static str {
    SWIPE_NAV_JS
}

/// Marks native gesture-phase delivery as available, injected before
/// [`page_script`] on platforms with a scroll-phase monitor (macOS). The
/// script then treats finger lift as authoritative and ignores momentum.
#[cfg(target_os = "macos")]
pub fn phase_flag_script() -> &'static str {
    "window.__wisSwipeHasPhase=true;"
}

/// A tiny per-tab script that publishes the indicator colour ahead of
/// [`page_script`]. `accent` is a CSS colour string, JSON-encoded so
/// it is always a safe string literal.
pub fn accent_script(accent: &str) -> String {
    format!(
        "window.__wisSwipeAccent={};",
        serde_json::to_string(accent).unwrap_or_default()
    )
}

fn nav_state_script(back: bool, fwd: bool) -> String {
    format!("window.__wisSwipeNavState&&window.__wisSwipeNavState({back},{fwd});")
}

/// Read the engine's `canGoBack`/`canGoForward` for a webview. `None` when
/// the platform read fails or times out (the closures hop to the UI thread;
/// from the load/title callbacks that hop is synchronous).
pub fn read_nav_state<R: tauri::Runtime>(
    webview: &tauri::webview::Webview<R>,
) -> Option<(bool, bool)> {
    #[cfg(target_os = "macos")]
    {
        use objc2::msg_send;
        use objc2::runtime::{AnyObject, Bool};

        let (tx, rx) = std::sync::mpsc::sync_channel::<(bool, bool)>(1);
        let _ = webview.with_webview(move |w| unsafe {
            let wk: *mut AnyObject = w.inner() as *mut AnyObject;
            if wk.is_null() {
                return;
            }

            let back: Bool = msg_send![wk, canGoBack];
            let fwd: Bool = msg_send![wk, canGoForward];
            let _ = tx.send((back.as_bool(), fwd.as_bool()));
        });
        rx.recv_timeout(std::time::Duration::from_millis(100)).ok()
    }

    #[cfg(windows)]
    {
        let (tx, rx) = std::sync::mpsc::sync_channel::<(bool, bool)>(1);
        crate::win_webview2::with_core_webview2(webview, move |core| {
            let mut back = windows::core::BOOL::default();
            let mut fwd = windows::core::BOOL::default();
            // SAFETY: runs on the UI thread via `with_core_webview2`.
            unsafe {
                let _ = core.CanGoBack(&mut back);
                let _ = core.CanGoForward(&mut fwd);
            }
            let _ = tx.send((back.as_bool(), fwd.as_bool()));
        });
        rx.recv_timeout(std::time::Duration::from_millis(100)).ok()
    }

    #[cfg(target_os = "linux")]
    {
        let (tx, rx) = std::sync::mpsc::sync_channel::<(bool, bool)>(1);
        let _ = webview.with_webview(move |w| {
            use webkit2gtk::WebViewExt;
            let wv = w.inner();
            let _ = tx.send((wv.can_go_back(), wv.can_go_forward()));
        });
        rx.recv_timeout(std::time::Duration::from_millis(100)).ok()
    }

    #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
    {
        let _ = webview;
        None
    }
}

/// Push the engine's `canGoBack`/`canGoForward` into a tab webview so the
/// swipe gesture only engages toward a direction with history. Called after
/// each page-load event and title change. Returns the values pushed so
/// callers can fold them into the tab model without a second engine read.
pub fn push_nav_state<R: tauri::Runtime>(
    webview: &tauri::webview::Webview<R>,
) -> Option<(bool, bool)> {
    let nav = read_nav_state(webview);
    if let Some((back, fwd)) = nav {
        let _ = webview.eval(nav_state_script(back, fwd));
    }

    nav
}

/// App-wide scroll-wheel phase monitor (macOS): forwards trackpad finger-down
/// and finger-lift transitions to every tab webview, so the swipe script can
/// commit exactly at lift and ignore the momentum tail.
#[cfg(target_os = "macos")]
pub fn install_phase_monitor(app: &tauri::AppHandle) {
    use block2::RcBlock;
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use tauri::Manager;

    // NSEventMaskScrollWheel = 1 << NSEventTypeScrollWheel(22).
    const MASK_SCROLL_WHEEL: u64 = 1 << 22;
    // NSEventPhase bits.
    const PHASE_BEGAN: u64 = 1 << 0;
    const PHASE_ENDED: u64 = 1 << 3;
    const PHASE_CANCELLED: u64 = 1 << 4;

    let app = app.clone();
    let handler = RcBlock::new(move |event: *mut AnyObject| -> *mut AnyObject {
        if event.is_null() {
            return event;
        }

        let phase: u64 = unsafe { msg_send![event, phase] };
        let js = if phase & PHASE_BEGAN != 0 {
            Some("window.__wisSwipePhase&&window.__wisSwipePhase('began');")
        } else if phase & (PHASE_ENDED | PHASE_CANCELLED) != 0 {
            Some("window.__wisSwipePhase&&window.__wisSwipePhase('ended');")
        } else {
            None
        };
        if let Some(js) = js {
            for (label, webview) in app.webviews() {
                if label.starts_with("tab-") {
                    let _ = webview.eval(js);
                }
            }
        }

        event
    });

    unsafe {
        let _token: *mut AnyObject = msg_send![
            objc2::class!(NSEvent),
            addLocalMonitorForEventsMatchingMask: MASK_SCROLL_WHEEL,
            handler: &*handler
        ];
    }

    std::mem::forget(handler);
}

#[cfg(not(target_os = "macos"))]
pub fn install_phase_monitor(_app: &tauri::AppHandle) {}

/// Update the swipe-nav indicator colour to the same colour that
/// themes the webview border: store it so new tab webviews bake it, and
/// push it into every live tab webview so the change is immediate. The chrome
/// calls this whenever the theme/accent changes.
#[tauri::command]
pub async fn set_swipe_accent(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::state::AppState>,
    color: String,
) -> wis_core::WisResult<()> {
    use tauri::Manager;

    if color.is_empty() || color.len() > 32 {
        return Ok(());
    }
    *state.swipe_accent.lock().expect("swipe_accent poisoned") = color.clone();

    let js = accent_script(&color);
    for (label, webview) in app.webviews() {
        if label.starts_with("tab-") {
            let _ = webview.eval(&js);
        }
    }

    Ok(())
}
