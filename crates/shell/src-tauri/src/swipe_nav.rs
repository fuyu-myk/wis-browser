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
//! The accent colour is baked per tab via a tiny `window.__wisSwipeAccent`
//! script (see `create_tab_webview`), built from the workspace's accent /
//! custom accent. Detection is wheel-based (WebKit exposes no native gesture
//! phase to JS), so feel constants live at the top of the script for tuning.

/// Builder-level script (run at document start) implementing the gesture + UI.
const SWIPE_NAV_JS: &str = r#"(function () {
  if (window.__wisSwipeNav) return;
  window.__wisSwipeNav = true;

  var THRESHOLD = 110;   // px of edge overscroll to arm a commit
  var R = 26;            // indicator radius (px); bulge depth = R, height = 2R
  var M = 4;             // viewBox padding so the stroke/apex is never clipped flat
  var END_DELAY = 90;    // ms with no wheel event = gesture ended

  function accent() { return window.__wisSwipeAccent || '#6b77f8'; }

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

  var active = null, endTimer = null;

  window.addEventListener('wheel', function (e) {
    if (!active && Math.abs(e.deltaX) <= Math.abs(e.deltaY)) return; // vertical
    if (window.history.length <= 1) return;

    if (!active) {
      if (e.deltaX < 0 && !canScroll(e.target, -1)) active = { dir: 'back', acc: 0, armed: false };
      else if (e.deltaX > 0 && !canScroll(e.target, 1)) active = { dir: 'forward', acc: 0, armed: false };
      else return;
      show(active.dir);
    }

    active.acc += active.dir === 'back' ? -e.deltaX : e.deltaX;
    if (active.acc < 0) active.acc = 0;
    e.preventDefault();
    e.stopPropagation();

    update(active.acc / THRESHOLD, active.dir);
    var armed = active.acc >= THRESHOLD;
    if (armed !== active.armed) { active.armed = armed; setArmed(armed); }

    clearTimeout(endTimer);
    endTimer = setTimeout(end, END_DELAY);
  }, { passive: false, capture: true });

  function end() {
    if (!active) return;
    var commit = active.armed, dir = active.dir;
    active = null;
    hide();
    if (commit) { if (dir === 'back') history.back(); else history.forward(); }
  }
})();"#;

/// The swipe-nav script, exposed so tab webviews bake it in as a builder-level
/// initialization script (runs at document start, before the first document).
pub fn page_script() -> &'static str {
    SWIPE_NAV_JS
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
