//! The page-side popout scripts: the builder-level document-start script that
//! exposes the `__wisPopout*` hooks, and the document-end tracker that pins the
//! video and mounts the control overlay. Pure string payloads, no native deps.

/// The page-side popout script, injected as a builder-level initialization
/// script (document start, main frame). Baked into the webview configuration
/// before the first document exists, so even an instantly-loading page (file
/// cache, localhost) can never race the injection. Top-level code only
/// registers listeners, so document-start execution is safe.
pub fn page_script() -> &'static str {
    POPOUT_JS
}

/// Injected page script (main frame, document end). Tracks the page's
/// dominant video and reports media-state changes (`playing` for the
/// automatic trigger, `present` for the manual ones); shows a small pop-out
/// button pinned to the video under the cursor; on demand
/// (`__wisPopoutEnter`) reparents the chosen video to the document root,
/// pins it across the viewport over a hidden body and black root, and
/// mounts the control overlay (X, click pause/play, double-click seek,
/// drag-to-move handoff, corner resize zones).
/// Muted, tiny, or detached videos never qualify as "playing", so muted
/// autoplay ads cannot trigger the automatic popout.
const POPOUT_JS: &str = r#"(function () {
  'use strict';
  if (window.__wisPopoutInstalled) return;
  window.__wisPopoutInstalled = true;
  if (window.top !== window) return;

  function post(msg) {
    try {
      var h = window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.wisPopout;
      if (h) h.postMessage(msg);
    } catch (e) {}
  }

  var current = null;
  var manualTarget = null;
  var manualAt = 0;
  var lastPlaying = false;
  var lastPresent = false;
  var reportTimer = null;
  var resumeTarget = null;
  var userPaused = false;

  function sizeOk(v) {
    if (!v || !v.isConnected || !v.videoWidth || !v.videoHeight) return false;
    var r = v.getBoundingClientRect();
    return r.width >= 180 && r.height >= 100;
  }
  function pick(playingOnly) {
    var vids = document.querySelectorAll('video');
    var best = null;
    var bestArea = 0;
    for (var i = 0; i < vids.length; i++) {
      var v = vids[i];
      if (!sizeOk(v)) continue;
      if (playingOnly && (v.paused || v.ended || v.muted)) continue;
      var r = v.getBoundingClientRect();
      var area = r.width * r.height;
      if (area > bestArea) { bestArea = area; best = v; }
    }
    return best;
  }
  function bestVideo() { return pick(true); }
  function anyVideo() { return pick(false); }
  function report() {
    if (reportTimer) return;
    reportTimer = setTimeout(function () {
      reportTimer = null;
      var v = bestVideo();
      if (v) current = v;
      var playing = !!v;
      var present = playing || !!anyVideo();
      if (playing !== lastPlaying || present !== lastPresent) {
        lastPlaying = playing;
        lastPresent = present;
        post({ kind: 'media', playing: playing, present: present });
      }
    }, 120);
  }
  ['play', 'playing', 'pause', 'ended', 'volumechange', 'emptied', 'loadedmetadata'].forEach(function (t) {
    document.addEventListener(t, report, true);
  });

  // Manual pop-out affordance: a small button pinned to the right edge of
  // the video under the cursor.
  var hoverBtn = null;
  var hoverFor = null;
  var hoverRaf = 0;
  function hoverButton() {
    if (hoverBtn) return hoverBtn;
    var b = document.createElement('button');
    b.id = '__wis-popout-hover';
    b.type = 'button';
    b.title = 'Pop out video';
    b.style.cssText = 'position:fixed;display:none;width:34px;height:34px;padding:0;border:0;border-radius:9px;background:rgba(15,15,20,0.62);color:#fff;cursor:pointer;z-index:2147483646;transition:background 0.15s ease;';
    var NS = 'http://www.w3.org/2000/svg';
    var svg = document.createElementNS(NS, 'svg');
    svg.setAttribute('width', '18');
    svg.setAttribute('height', '18');
    svg.setAttribute('viewBox', '0 0 18 18');
    svg.setAttribute('fill', 'none');
    svg.setAttribute('stroke', 'currentColor');
    svg.setAttribute('stroke-width', '1.6');
    svg.style.cssText = 'display:block;margin:0 auto;';
    var outer = document.createElementNS(NS, 'rect');
    outer.setAttribute('x', '1.5');
    outer.setAttribute('y', '3');
    outer.setAttribute('width', '15');
    outer.setAttribute('height', '12');
    outer.setAttribute('rx', '2');
    var inner = document.createElementNS(NS, 'rect');
    inner.setAttribute('x', '8.5');
    inner.setAttribute('y', '9');
    inner.setAttribute('width', '6');
    inner.setAttribute('height', '4.5');
    inner.setAttribute('rx', '1');
    inner.setAttribute('fill', 'currentColor');
    inner.setAttribute('stroke', 'none');
    svg.appendChild(outer);
    svg.appendChild(inner);
    b.appendChild(svg);
    b.addEventListener('mouseenter', function () { b.style.background = 'rgba(15,15,20,0.85)'; });
    b.addEventListener('mouseleave', function () { b.style.background = 'rgba(15,15,20,0.62)'; });
    b.addEventListener('click', function (e) {
      e.preventDefault();
      e.stopPropagation();
      if (hoverFor) {
        manualTarget = hoverFor;
        manualAt = Date.now();
      }
      hideHover();
      post({ kind: 'request' });
    });
    document.documentElement.appendChild(b);
    hoverBtn = b;
    return b;
  }
  function hideHover() {
    if (hoverBtn) hoverBtn.style.display = 'none';
    hoverFor = null;
  }
  function topmostAt(x, y) {
    var stack = document.elementsFromPoint(x, y);
    for (var i = 0; i < stack.length; i++) {
      var el = stack[i];
      if (hoverBtn && (el === hoverBtn || hoverBtn.contains(el))) continue;
      return el;
    }
    return null;
  }
  function videoUnoccluded(target, vr, x, y) {
    var top = topmostAt(x, y);
    if (!top) return false;
    if (top === target || target.contains(top) || top.contains(target)) return true;
    var tr = top.getBoundingClientRect();
    return tr.width >= vr.width * 0.9 && tr.height >= vr.height * 0.9;
  }
  function placeHover(cx, cy) {
    if (active) { hideHover(); return; }
    var vids = document.querySelectorAll('video');
    var target = null;
    var targetRect = null;
    var bestArea = 0;
    for (var i = 0; i < vids.length; i++) {
      var v = vids[i];
      if (!sizeOk(v)) continue;
      var r = v.getBoundingClientRect();
      if (cx < r.left || cx > r.right || cy < r.top || cy > r.bottom) continue;
      var area = r.width * r.height;
      if (area > bestArea) { bestArea = area; target = v; targetRect = r; }
    }
    if (!target) { hideHover(); return; }
    // Hide when a page popup occludes the video where the button would land.
    var bx = targetRect.right - 27;
    var by = targetRect.top + targetRect.height / 2;
    if (!videoUnoccluded(target, targetRect, bx, by)) { hideHover(); return; }
    var b = hoverButton();
    b.style.left = Math.round(targetRect.right - 44) + 'px';
    b.style.top = Math.round(targetRect.top + (targetRect.height - 34) / 2) + 'px';
    b.style.display = 'block';
    hoverFor = target;
  }
  document.addEventListener('mousemove', function (e) {
    if (hoverRaf) return;
    var cx = e.clientX;
    var cy = e.clientY;
    hoverRaf = requestAnimationFrame(function () {
      hoverRaf = 0;
      placeHover(cx, cy);
    });
  }, { capture: true, passive: true });
  document.addEventListener('scroll', hideHover, { capture: true, passive: true });

  var CSS =
    'html.__wis-popout-mode{background:#000!important;transform:none!important;filter:none!important;}' +
    'html.__wis-popout-mode body{visibility:hidden!important;background:#000!important;}' +
    'video.__wis-popout-video{position:fixed!important;inset:0 auto auto 0!important;width:100vw!important;height:100vh!important;max-width:none!important;max-height:none!important;min-width:0!important;min-height:0!important;margin:0!important;padding:0!important;border:0!important;border-radius:0!important;transform:none!important;visibility:visible!important;opacity:1!important;display:block!important;z-index:2147483644!important;object-fit:contain!important;background:#000!important;}' +
    'video.__wis-popout-video::-webkit-media-controls{display:none!important;}' +
    'video.__wis-popout-video::-webkit-media-controls-enclosure{display:none!important;}' +
    '#__wis-popout-overlay{position:fixed;inset:0;z-index:2147483645;cursor:default;-webkit-user-select:none;user-select:none;font-family:-apple-system,BlinkMacSystemFont,sans-serif;}' +
    '#__wis-popout-x{position:absolute;top:10px;right:10px;width:28px;height:28px;border:0;border-radius:50%;background:rgba(15,15,20,0.62);color:#fff;font-size:17px;line-height:28px;text-align:center;padding:0;cursor:pointer;opacity:0;transition:opacity 0.18s ease,background 0.18s ease;z-index:3;}' +
    '#__wis-popout-overlay:hover #__wis-popout-x,#__wis-popout-overlay.is-hover #__wis-popout-x,#__wis-popout-overlay.is-fresh #__wis-popout-x{opacity:1;}' +
    '#__wis-popout-x:hover{background:rgba(15,15,20,0.85);}' +
    '#__wis-popout-vol{position:absolute;left:10px;bottom:10px;display:flex;align-items:center;gap:6px;height:28px;padding:0 10px 0 3px;border-radius:14px;background:rgba(15,15,20,0.62);opacity:0;transition:opacity 0.18s ease;z-index:3;}' +
    '#__wis-popout-overlay:hover #__wis-popout-vol,#__wis-popout-overlay.is-hover #__wis-popout-vol,#__wis-popout-overlay.is-fresh #__wis-popout-vol{opacity:1;}' +
    '#__wis-popout-vol button{width:24px;height:24px;border:0;border-radius:50%;background:transparent;color:#fff;cursor:pointer;padding:0;display:grid;place-items:center;}' +
    '#__wis-popout-vol button:hover{background:rgba(255,255,255,0.18);}' +
    '#__wis-popout-vol button svg{width:15px;height:15px;display:block;}' +
    '#__wis-popout-vol input{-webkit-appearance:none;appearance:none;width:72px;height:3px;border-radius:2px;background:rgba(255,255,255,0.35);outline:none;cursor:pointer;margin:0;}' +
    '#__wis-popout-vol input::-webkit-slider-thumb{-webkit-appearance:none;appearance:none;width:11px;height:11px;border-radius:50%;background:#fff;border:0;}' +
    '.__wis-popout-corner{position:absolute;width:18px;height:18px;z-index:2;}' +
    '.__wis-popout-corner[data-corner=nw]{top:0;left:0;cursor:nwse-resize;}' +
    '.__wis-popout-corner[data-corner=se]{bottom:0;right:0;cursor:nwse-resize;}' +
    '.__wis-popout-corner[data-corner=ne]{top:0;right:0;cursor:nesw-resize;}' +
    '.__wis-popout-corner[data-corner=sw]{bottom:0;left:0;cursor:nesw-resize;}' +
    '#__wis-popout-paused{position:absolute;top:50%;left:50%;width:64px;height:64px;margin:-32px 0 0 -32px;border-radius:50%;background:rgba(15,15,20,0.55);opacity:0;transition:opacity 0.18s ease;pointer-events:none;z-index:1;}' +
    '#__wis-popout-paused::after{content:\'\';position:absolute;top:50%;left:50%;margin:-13px 0 0 -8px;border-left:22px solid #fff;border-top:13px solid transparent;border-bottom:13px solid transparent;}' +
    '#__wis-popout-badge{position:absolute;top:50%;left:50%;transform:translate(-50%,-50%);padding:10px 16px;border-radius:18px;background:rgba(15,15,20,0.62);color:#fff;font-size:15px;font-weight:600;opacity:0;pointer-events:none;z-index:2;}' +
    '#__wis-popout-badge.is-on{animation:__wis-popout-fade 0.7s ease forwards;}' +
    '@keyframes __wis-popout-fade{0%{opacity:1;}70%{opacity:1;}100%{opacity:0;}}';

  var active = null;

  window.__wisPopoutEnter = function () {
    if (active) return;
    hideHover();
    var v = null;
    if (manualTarget && manualTarget.isConnected && manualTarget.videoWidth && Date.now() - manualAt < 6000) {
      v = manualTarget;
    }
    manualTarget = null;
    if (!v) v = bestVideo();
    if (!v && current && current.isConnected && current.videoWidth) v = current;
    if (!v) v = anyVideo();
    if (!v) return;
    userPaused = false;
    resumeTarget = !v.paused && !v.ended ? v : null;

    var style = document.createElement('style');
    style.id = '__wis-popout-style';
    style.textContent = CSS;
    var overlay = document.createElement('div');
    overlay.id = '__wis-popout-overlay';
    overlay.className = 'is-fresh';

    ['nw', 'ne', 'sw', 'se'].forEach(function (c) {
      var zone = document.createElement('div');
      zone.className = '__wis-popout-corner';
      zone.setAttribute('data-corner', c);
      overlay.appendChild(zone);
    });
    var x = document.createElement('button');
    x.id = '__wis-popout-x';
    x.type = 'button';
    x.textContent = '×';
    var paused = document.createElement('div');
    paused.id = '__wis-popout-paused';
    var badge = document.createElement('div');
    badge.id = '__wis-popout-badge';
    var vol = document.createElement('div');
    vol.id = '__wis-popout-vol';
    var volBtn = document.createElement('button');
    volBtn.type = 'button';
    var volOn = volIcon(false);
    var volOff = volIcon(true);
    volBtn.appendChild(volOn);
    volBtn.appendChild(volOff);
    var volSlider = document.createElement('input');
    volSlider.type = 'range';
    volSlider.min = '0';
    volSlider.max = '1';
    volSlider.step = '0.01';
    volSlider.setAttribute('aria-label', 'Volume');
    vol.appendChild(volBtn);
    vol.appendChild(volSlider);
    overlay.appendChild(x);
    overlay.appendChild(paused);
    overlay.appendChild(badge);
    overlay.appendChild(vol);

    // Reparent the video to <html> while popped.
    // The original slot is restored on exit.
    var restore = { parent: v.parentNode, next: v.nextSibling };
    v.classList.add('__wis-popout-video');
    document.documentElement.classList.add('__wis-popout-mode');
    document.documentElement.appendChild(style);
    document.documentElement.appendChild(v);
    document.documentElement.appendChild(overlay);

    active = {
      video: v, style: style, overlay: overlay, restore: restore,
      x: x, paused: paused, badge: badge,
      vol: vol, volBtn: volBtn, volOn: volOn, volOff: volOff,
      volSlider: volSlider, volDragging: false,
      watch: 0, freshTimer: 0, clickTimer: 0, lastClick: 0, down: null,
      aspectTimer: 0
    };

    syncPaused();
    syncVolume();
    v.addEventListener('play', syncPaused);
    v.addEventListener('pause', syncPaused);
    v.addEventListener('resize', onAspect);
    v.addEventListener('volumechange', syncVolume);
    volBtn.addEventListener('click', onVolButton);
    volSlider.addEventListener('input', onVolSlider);
    volSlider.addEventListener('pointerdown', function () { if (active) active.volDragging = true; });
    volSlider.addEventListener('pointerup', function () { if (active) active.volDragging = false; });
    volSlider.addEventListener('pointercancel', function () { if (active) active.volDragging = false; });
    overlay.addEventListener('pointerdown', onDown);
    overlay.addEventListener('pointermove', onMove);
    overlay.addEventListener('pointerup', onUp);
    overlay.addEventListener('pointercancel', onCancel);
    overlay.addEventListener('wheel', onWheel, { passive: false });
    overlay.addEventListener('contextmenu', prevent);
    x.addEventListener('click', onClose);
    window.addEventListener('pagehide', onPagehide);

    active.freshTimer = setTimeout(function () { overlay.classList.remove('is-fresh'); }, 1800);
    active.watch = setInterval(watch, 1500);

    post({ kind: 'entered', aspect: v.videoWidth / v.videoHeight });
  };

  window.__wisPopoutExit = function () {
    if (active) {
      var v = active.video;
      resumeTarget = !v.paused && !v.ended ? v : null;
    }
    teardown(null);
  };

  // Moving the webview between windows pauses media engine-side; the shell
  // calls this after each move to kick a video that was playing back into
  // playback. The pause can land a beat after the move, so it retries over
  // a short window; a user-intended pause (X, click) is never overridden.
  window.__wisPopoutResume = function () {
    var v = resumeTarget;
    resumeTarget = null;
    if (!v) return;
    var deadline = Date.now() + 1200;
    function kick() {
      if (!v.isConnected || v.ended || userPaused) return;
      if (v.paused) {
        var p = v.play();
        if (p && p.catch) p.catch(function () {});
      }
      if (Date.now() < deadline) setTimeout(kick, 200);
    }
    kick();
  };

  window.__wisPopoutHover = function (on) {
    if (!active) return;
    active.overlay.classList.toggle('is-hover', !!on);
  };

  function teardown(notifyKind) {
    if (!active) return;
    var a = active;
    active = null;
    clearInterval(a.watch);
    clearTimeout(a.freshTimer);
    clearTimeout(a.clickTimer);
    clearTimeout(a.aspectTimer);
    a.video.removeEventListener('play', syncPaused);
    a.video.removeEventListener('pause', syncPaused);
    a.video.removeEventListener('resize', onAspect);
    a.video.removeEventListener('volumechange', syncVolume);
    window.removeEventListener('pagehide', onPagehide);
    try {
      var r = a.restore;
      if (r && r.parent && r.parent.isConnected && a.video.isConnected) {
        if (r.next && r.next.parentNode === r.parent) {
          r.parent.insertBefore(a.video, r.next);
        } else {
          r.parent.appendChild(a.video);
        }
      }
    } catch (e) {}
    try { a.video.classList.remove('__wis-popout-video'); } catch (e) {}
    try { document.documentElement.classList.remove('__wis-popout-mode'); } catch (e) {}
    [a.style, a.overlay].forEach(function (n) {
      try { n.remove(); } catch (e) {}
    });
    if (notifyKind) post({ kind: notifyKind });
  }

  function watch() {
    if (!active) return;
    if (!active.video.isConnected) { teardown('lost'); return; }
    if (!active.style.isConnected) document.documentElement.appendChild(active.style);
    if (!active.overlay.isConnected) document.documentElement.appendChild(active.overlay);
    var root = document.documentElement.classList;
    if (!root.contains('__wis-popout-mode')) root.add('__wis-popout-mode');
    if (!active.video.classList.contains('__wis-popout-video')) {
      active.video.classList.add('__wis-popout-video');
    }
  }

  function onPagehide() { teardown('lost'); }
  function prevent(e) { e.preventDefault(); }
  function onAspect() {
    if (!active) return;
    clearTimeout(active.aspectTimer);
    active.aspectTimer = setTimeout(function () {
      if (!active) return;
      var v = active.video;
      if (v.videoWidth && v.videoHeight) {
        post({ kind: 'aspect', aspect: v.videoWidth / v.videoHeight });
      }
    }, 150);
  }
  function syncPaused() {
    if (!active) return;
    active.paused.style.opacity = active.video.paused ? '1' : '0';
  }
  function volIcon(slashed) {
    var NS = 'http://www.w3.org/2000/svg';
    var svg = document.createElementNS(NS, 'svg');
    svg.setAttribute('viewBox', '0 0 24 24');
    svg.setAttribute('fill', 'none');
    svg.setAttribute('stroke', 'currentColor');
    svg.setAttribute('stroke-width', '2');
    svg.setAttribute('stroke-linecap', 'round');
    svg.setAttribute('stroke-linejoin', 'round');
    var paths = slashed
      ? ['M11 4.7 6.6 8.2H3v7.6h3.6L11 19.3z', 'm21 9-6 6', 'm15 9 6 6']
      : ['M11 4.7 6.6 8.2H3v7.6h3.6L11 19.3z', 'M15.5 8.5a5 5 0 0 1 0 7', 'M18.6 5.4a9 9 0 0 1 0 13.2'];
    for (var i = 0; i < paths.length; i++) {
      var p = document.createElementNS(NS, 'path');
      p.setAttribute('d', paths[i]);
      svg.appendChild(p);
    }
    return svg;
  }
  // Mirrors the element's live volume/mute (the page player drives the same
  // element, so its slider and this one stay in lockstep both ways).
  function syncVolume() {
    if (!active) return;
    var v = active.video;
    var silent = v.muted || v.volume === 0;
    active.volOn.style.display = silent ? 'none' : 'block';
    active.volOff.style.display = silent ? 'block' : 'none';
    active.volBtn.title = v.muted ? 'Unmute' : 'Mute';
    active.volBtn.setAttribute('aria-label', v.muted ? 'Unmute' : 'Mute');
    if (!active.volDragging) active.volSlider.value = String(v.volume);
  }
  function onVolButton(e) {
    e.preventDefault();
    e.stopPropagation();
    if (!active) return;
    active.video.muted = !active.video.muted;
  }
  function onVolSlider() {
    if (!active) return;
    var v = active.video;
    var value = parseFloat(active.volSlider.value);
    if (!isFinite(value)) return;
    v.volume = Math.min(1, Math.max(0, value));
    if (v.volume > 0 && v.muted) v.muted = false;
  }
  function onWheel(e) {
    e.preventDefault();
    if (!active || !active.vol.contains(e.target)) return;
    var v = active.video;
    var next = Math.min(1, Math.max(0, v.volume + (e.deltaY < 0 ? 0.05 : -0.05)));
    v.volume = next;
    if (next > 0 && v.muted) v.muted = false;
  }
  function onClose(e) {
    e.preventDefault();
    e.stopPropagation();
    if (!active) return;
    userPaused = true;
    resumeTarget = null;
    try { active.video.pause(); } catch (err) {}
    teardown('close');
  }
  function onDown(e) {
    if (!active || e.button !== 0) return;
    if (active.x.contains(e.target) || active.vol.contains(e.target)) return;
    var corner = e.target && e.target.getAttribute ? e.target.getAttribute('data-corner') : null;
    active.down = { sx: e.screenX, sy: e.screenY, corner: corner, moved: false, ax: 0, ay: 0, raf: 0 };
    try { active.overlay.setPointerCapture(e.pointerId); } catch (err) {}
    e.preventDefault();
  }
  function onMove(e) {
    if (!active || !active.down) return;
    var d = active.down;
    if (!d.moved) {
      if (Math.abs(e.screenX - d.sx) < 4 && Math.abs(e.screenY - d.sy) < 4) return;
      d.moved = true;
      if (!d.corner) {
        active.down = null;
        post({ kind: 'dragstart' });
        return;
      }
      d.sx = e.screenX;
      d.sy = e.screenY;
      return;
    }
    if (!d.corner) return;
    d.ax += e.screenX - d.sx;
    d.ay += e.screenY - d.sy;
    d.sx = e.screenX;
    d.sy = e.screenY;
    if (!d.raf) {
      d.raf = requestAnimationFrame(function () {
        if (!active || active.down !== d) return;
        d.raf = 0;
        if (d.ax || d.ay) {
          post({ kind: 'resize', corner: d.corner, dx: d.ax, dy: d.ay });
          d.ax = 0;
          d.ay = 0;
        }
      });
    }
  }
  function onUp(e) {
    if (!active || !active.down) return;
    var d = active.down;
    active.down = null;
    if (d.raf) cancelAnimationFrame(d.raf);
    if (d.moved) {
      if (d.corner && (d.ax || d.ay)) post({ kind: 'resize', corner: d.corner, dx: d.ax, dy: d.ay });
      return;
    }
    handleClick(e);
  }
  function onCancel() { if (active) active.down = null; }
  function handleClick(e) {
    if (!active) return;
    var now = Date.now();
    var third = active.overlay.clientWidth / 3;
    var cx = e.clientX;
    if (now - active.lastClick < 300) {
      active.lastClick = 0;
      clearTimeout(active.clickTimer);
      active.clickTimer = 0;
      if (cx < third) seek(-10);
      else if (cx > 2 * third) seek(10);
      else toggle();
      return;
    }
    active.lastClick = now;
    active.clickTimer = setTimeout(function () {
      if (!active) return;
      active.clickTimer = 0;
      toggle();
    }, 300);
  }
  function toggle() {
    if (!active) return;
    var v = active.video;
    if (v.paused) {
      userPaused = false;
      var p = v.play();
      if (p && p.catch) p.catch(function () {});
    } else {
      userPaused = true;
      v.pause();
    }
  }
  function seek(delta) {
    if (!active) return;
    var v = active.video;
    var t = v.currentTime + delta;
    if (t < 0) t = 0;
    if (isFinite(v.duration) && t > v.duration) t = v.duration;
    try { v.currentTime = t; } catch (e) { return; }
    flash(delta < 0 ? '-10s' : '+10s');
  }
  function flash(text) {
    if (!active) return;
    var b = active.badge;
    b.textContent = text;
    b.classList.remove('is-on');
    void b.offsetWidth;
    b.classList.add('is-on');
  }
})();"#;
