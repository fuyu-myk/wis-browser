//! The scripts `address_field` places in pages: classifying and showing
//! address fields, reporting focus on one, and filling a saved address.

/// Classifies form controls by their `autocomplete` tokens and judges whether
/// a person can see them. Defines `addressField`, `addressSection`,
/// `shownField`, `offerKinds` and the helpers they use for the scripts it is
/// placed in.
pub(super) const CLASSIFY_JS: &str = r#"
var ADDRESS_KINDS = {
  'name': 'name',
  'given-name': 'given',
  'family-name': 'family',
  'organization': 'organization',
  'street-address': 'street',
  'address-line1': 'line1',
  'address-line2': 'line2',
  'address-line3': 'line3',
  'address-level2': 'city',
  'address-level1': 'region',
  'postal-code': 'postal',
  'country': 'country',
  'country-name': 'country',
  'tel': 'tel',
  'tel-national': 'tel',
  'email': 'email'
};
var PLACE_KINDS = ['street', 'line1', 'city', 'region', 'postal', 'country'];
var INPUT_TYPES = { '': 1, text: 1, search: 2, tel: 4, number: 8, email: 16 };
// The autofill field names that are not an address's.
var OTHER_FIELDS = ['username', 'new-password', 'current-password', 'one-time-code',
  'cc-name', 'cc-given-name', 'cc-additional-name', 'cc-family-name', 'cc-number', 'cc-exp',
  'cc-exp-month', 'cc-exp-year', 'cc-csc', 'cc-type', 'transaction-currency',
  'transaction-amount', 'language', 'bday', 'bday-day', 'bday-month', 'bday-year', 'sex',
  'url', 'photo', 'impp', 'honorific-prefix', 'additional-name', 'honorific-suffix',
  'nickname', 'organization-title', 'address-level3', 'address-level4', 'tel-country-code',
  'tel-area-code', 'tel-local', 'tel-local-prefix', 'tel-local-suffix', 'tel-extension'];
var MAX_FIELDS = 200;
var PAGE_PARTS = 'header, footer, aside, nav, dialog, [role="search"], [role="dialog"]';

function docProp(name) {
  try {
    return Object.getOwnPropertyDescriptor(Document.prototype, name).get.call(document);
  } catch (e) {
    return document[name];
  }
}

function query(root, selector) {
  try {
    var proto = root === document ? Document.prototype : Element.prototype;
    return proto.querySelectorAll.call(root, selector);
  } catch (e) {
    return root.querySelectorAll(selector);
  }
}

function own(map, key) {
  return Object.prototype.hasOwnProperty.call(map, key) ? map[key] : undefined;
}

// The control type bit the patterns use, or 0 for a control no address goes in.
function controlType(el) {
  if (!el || el.disabled || el.readOnly) return 0;
  if (el.tagName === 'INPUT') {
    return own(INPUT_TYPES, String(el.getAttribute('type') || '').toLowerCase()) || 0;
  }
  if (el.tagName === 'SELECT') return el.multiple ? 0 : 32;
  return el.tagName === 'TEXTAREA' ? 64 : 0;
}

function tokensOf(el) {
  var tokens = String(el.getAttribute('autocomplete') || '')
    .toLowerCase()
    .trim()
    .split(/\s+/)
    .filter(Boolean);
  if (tokens[tokens.length - 1] === 'webauthn') tokens.pop();
  return tokens;
}

function addressField(el) {
  if (!controlType(el)) return null;
  var tokens = tokensOf(el);
  var kind = own(ADDRESS_KINDS, tokens.pop() || '');
  if (!kind) return null;
  var section = tokens.filter(function (token) {
    return token.indexOf('section-') === 0 || token === 'shipping' || token === 'billing';
  });
  return { kind: kind, section: section.join(' ') };
}

// Whether `el` is read from its words: no autofill field name of its own.
function guessable(el) {
  if (!controlType(el)) return false;
  var last = tokensOf(el).pop() || '';
  return !own(ADDRESS_KINDS, last) && OTHER_FIELDS.indexOf(last) < 0;
}

// The form `el` belongs to, or for a field outside any form, the part of the
// page it sits in: a header, footer, aside, nav or dialog, else the page.
function groupOf(el) {
  if (el.form) return el.form;
  var part = null;
  try {
    part = Element.prototype.closest.call(el, PAGE_PARTS);
  } catch (e) {
    part = typeof el.closest === 'function' ? el.closest(PAGE_PARTS) : null;
  }
  return part || document;
}

// Every address field in the group `root`, tagged or read from its words,
// with its section.
function addressFields(root) {
  var all = (root.tagName === 'FORM' && read('HTMLFormElement', 'elements', root)) ||
    query(root, 'input, select, textarea');
  var found = [];
  var guesses = [];
  var carded = false;
  for (var i = 0; i < all.length && i < MAX_FIELDS; i++) {
    var el = all[i];
    if (groupOf(el) !== root) continue;
    var tagged = addressField(el);
    if (tagged) {
      found.push({ el: el, kind: tagged.kind, section: tagged.section, guessed: false });
    } else if (guessable(el)) {
      guesses.push(el);
    } else if ((tokensOf(el).pop() || '').indexOf('cc-') === 0) {
      carded = true;
    }
  }
  return found.concat(guessKinds(guesses, carded));
}

function addressSection(el) {
  if (!controlType(el)) return null;
  var fields = addressFields(groupOf(el));
  var mine = null;
  for (var i = 0; i < fields.length; i++) {
    if (fields[i].el === el) mine = fields[i];
  }
  if (!mine) return null;
  return fields.filter(function (field) {
    return field.section === mine.section;
  });
}

// A section read from words alone needs three kinds of field, as in Chromium.
function worthOffering(kinds, guessed) {
  if (guessed) return kinds.length >= 3;
  return kinds.length >= 3 || kinds.some(function (kind) {
    return PLACE_KINDS.indexOf(kind) >= 0;
  });
}

function read(type, name, node) {
  try {
    return Object.getOwnPropertyDescriptor(globalThis[type].prototype, name).get.call(node);
  } catch (e) {
    return node[name];
  }
}

// Up the rendered tree: into the slot a field is shown in, and out of a
// shadow root to its host.
function parentOf(node) {
  var slot = read('Element', 'assignedSlot', node);
  if (slot) return slot;
  var up = read('Node', 'parentElement', node);
  if (up) return up;
  var root = read('Node', 'parentNode', node);
  return root && root.nodeType === 11 ? read('ShadowRoot', 'host', root) : null;
}

function holds(node, other) {
  try {
    return Node.prototype.contains.call(node, other);
  } catch (e) {
    return node === other || node.contains(other);
  }
}

function boxOf(el) {
  try {
    return Element.prototype.getBoundingClientRect.call(el);
  } catch (e) {
    return el.getBoundingClientRect();
  }
}

// The alpha of a computed colour, in any of its syntaxes.
function alphaOf(color) {
  var c = String(color || '').trim();
  if (c === 'transparent') return 0;
  var inner = /\(([^)]*)\)$/.exec(c);
  if (!inner) return 1;
  var parts = inner[1].split('/');
  var alpha = parts.length > 1 ? parts[1] : inner[1].split(',')[3];
  var n = parseFloat(alpha);
  if (isNaN(n)) return 1;
  return /%\s*$/.test(alpha) ? n / 100 : n;
}

function clipped(style) {
  return (style.clipPath && style.clipPath !== 'none') ||
    (style.webkitClipPath && style.webkitClipPath !== 'none') ||
    (style.clip && style.clip !== 'auto') ||
    (style.maskImage && style.maskImage !== 'none') ||
    (style.webkitMaskImage && style.webkitMaskImage !== 'none') ||
    (style.mixBlendMode && style.mixBlendMode !== 'normal') ||
    /opacity|blur|url\(/.test(style.filter || '');
}

function hides(overflow) {
  return /hidden|clip/.test(overflow || '');
}

function small(box) {
  return box.right - box.left < 10 || box.bottom - box.top < 10;
}

function reversed(s, axis) {
  if (!/^horizontal/.test(s.writingMode || 'horizontal-tb')) return true;
  var flex = /flex/.test(s.display || '') ? s.flexDirection : '';
  if (axis === 'X') return s.direction === 'rtl' || flex === 'row-reverse';
  return flex === 'column-reverse';
}

// Whether `s` makes its box the containing block of a positioned field
// inside it, other than by its own position.
function containsBlock(s) {
  var none = function (value) { return !value || value === 'none'; };
  return !none(s.transform) || !none(s.perspective) || !none(s.filter) ||
    !none(s.backdropFilter) || !none(s.webkitBackdropFilter) ||
    /layout|paint|strict|content/.test(s.contain || '') ||
    /transform|perspective|filter/.test(s.willChange || '') ||
    /size/.test(s.containerType || '');
}

// The span `[lo, hi]` as far as `node`'s box `[from, to]` lets it show along
// one axis: moved to the box's middle when scrolling the box reaches it, else
// cut to the box where it scrolls, clips or contains paint.
function along(node, s, axis, lo, hi, from, to, pinned) {
  var overflow = axis === 'X' ? s.overflowX : s.overflowY;
  var scrolls = /auto|scroll/.test(overflow || '');
  if (scrolls && pinned) return [lo, hi, false];
  if (scrolls && !reversed(s, axis) && (lo < from || hi > to)) {
    var start = from +
      Number(read('Element', axis === 'X' ? 'clientLeft' : 'clientTop', node)) -
      Number(read('Element', axis === 'X' ? 'scrollLeft' : 'scrollTop', node));
    var end = start + Number(read('Element', axis === 'X' ? 'scrollWidth' : 'scrollHeight', node));
    if (lo >= start - 1 && hi <= end + 1) {
      var size = Math.min(hi - lo, to - from);
      var mid = (from + to) / 2;
      return [mid - size / 2, mid + size / 2, true];
    }
  }
  if (scrolls || hides(overflow) || /paint|strict|content/.test(s.contain || '')) {
    return [Math.max(lo, from), Math.min(hi, to), false];
  }
  return [lo, hi, false];
}

// Whether a person can see `el`. The field the user is in (`anchor`) skips
// the last test.
function shownField(el, anchor) {
  if (el.getAttribute('aria-hidden') === 'true') return false;
  if (typeof el.checkVisibility === 'function' && !el.checkVisibility({
    opacityProperty: true,
    visibilityProperty: true,
    checkOpacity: true,
    checkVisibilityCSS: true
  })) return false;
  var box = boxOf(el);
  if (box.width < 10 || box.height < 10) return false;
  var style = getComputedStyle(el);
  if (parseFloat(style.fontSize) < 6 || alphaOf(style.color) < 0.3 ||
      alphaOf(style.webkitTextFillColor) < 0.3) return false;

  var root = docProp('documentElement');
  var body = docProp('body');
  var rootStyle = getComputedStyle(root);
  var owner = body && rootStyle.overflowX === 'visible' && rootStyle.overflowY === 'visible'
    ? body : root;
  var page = owner === root ? rootStyle : getComputedStyle(owner);

  // `escape` is set while the field is positioned out of the boxes around it.
  var seen = { left: box.left, top: box.top, right: box.right, bottom: box.bottom };
  var opacity = 1;
  var escape = '';
  var mover = null;
  for (var node = el; node && node.nodeType === 1; node = parentOf(node)) {
    var s = node === el ? style : getComputedStyle(node);
    if (s.display === 'none' || s.visibility === 'hidden' || s.visibility === 'collapse') return false;
    opacity *= Number(s.opacity);
    if (!(opacity >= 0.5) || clipped(s)) return false;
    if (node !== el && escape && (containsBlock(s) ||
        (escape === 'absolute' && s.position && s.position !== 'static'))) escape = '';
    if (node !== el && node !== root && node !== owner && s.display !== 'contents') {
      var r = boxOf(node);
      var x = along(node, s, 'X', seen.left, seen.right, r.left, r.right, escape !== '');
      var y = along(node, s, 'Y', seen.top, seen.bottom, r.top, r.bottom, escape !== '');
      seen = { left: x[0], right: x[1], top: y[0], bottom: y[1] };
      if (x[2] || y[2]) mover = node;
      if (small(seen)) return false;
    }
    if (s.position === 'fixed') escape = 'fixed';
    else if (s.position === 'absolute' && escape === '') escape = 'absolute';
  }
  var fixed = escape === 'fixed';

  var sx = window.scrollX, sy = window.scrollY;
  var wide = !fixed && !hides(page.overflowX);
  var tall = !fixed && !hides(page.overflowY);
  if (small({
    left: Math.max(seen.left, wide ? -sx : 0),
    top: Math.max(seen.top, tall ? -sy : 0),
    right: Math.min(seen.right, wide ? root.scrollWidth - sx : window.innerWidth),
    bottom: Math.min(seen.bottom, tall ? root.scrollHeight - sy : window.innerHeight)
  })) return false;

  if (el === anchor) return true;
  var cx = (seen.left + seen.right) / 2;
  var cy = (seen.top + seen.bottom) / 2;
  if (cx >= 0 && cy >= 0 && cx < window.innerWidth && cy < window.innerHeight) {
    var hit = document.elementFromPoint(cx, cy);
    if (hit && !holds(mover || el, hit)) return false;
  }
  return true;
}

function untouched(select) {
  var picked = select.options[select.selectedIndex];
  if (!select.value || !picked || picked.defaultSelected) return true;
  for (var i = 0; i < select.options.length; i++) {
    if (select.options[i].defaultSelected) return false;
  }
  return select.selectedIndex === 0;
}

// The kinds of fields a fill from `el` would reach, or null when `el` is not a
// shown address field or its section is not worth an address. After a
// country not yet chosen, hidden address fields count too.
function offerKinds(el) {
  if (!addressField(el) && !(guessable(el) && maybeAddress(el))) return null;
  if (!shownField(el, el)) return null;
  var section = addressSection(el);
  if (!section) return null;
  var country = -1;
  section.forEach(function (field, i) {
    if (country >= 0 || field.kind !== 'country') return;
    if (field.el.tagName === 'SELECT' ? untouched(field.el) : !field.el.value) country = i;
  });
  var shown = [];
  var kinds = [];
  section.forEach(function (field, i) {
    var list = field.el.tagName === 'SELECT' && (field.kind === 'country' || field.kind === 'region');
    var later = country >= 0 && i > country && PLACE_KINDS.indexOf(field.kind) >= 0;
    var visible = shownField(field.el, el);
    if (visible && shown.indexOf(field.kind) < 0) shown.push(field.kind);
    if ((visible || list || later) && kinds.indexOf(field.kind) < 0) kinds.push(field.kind);
  });
  return worthOffering(shown, section[0].guessed) ? kinds : null;
}
"#;

/// Fills the section of the field the card was shown for (`__wisAddressAnchor`)
/// with the values `V`, once the page is still on `ORIGIN`, still in that
/// field and still on the report `TOKEN` came with. A list still on the page's
/// own default counts as empty, as does a field still holding an earlier
/// fill's value. The country goes first, and a form that changes afterwards
/// is filled again as it settles.
pub(super) const FILL_JS: &str = r#"
try {
  if (location.origin !== ORIGIN || window.__wisAddressToken !== TOKEN) return;
  var anchor = window.__wisAddressAnchor;
  if (!anchor || !anchor.isConnected || anchor !== docProp('activeElement')) return;
  var lines = V.street ? V.street.split('\n') : [];

  var valueFor = function (kind, el) {
    switch (kind) {
      case 'name': return [V.given, V.family].filter(Boolean).join(' ');
      case 'given': return V.given;
      case 'family': return V.family;
      case 'organization': return V.organization;
      case 'street': return el.tagName === 'TEXTAREA' ? lines.join('\n') : lines.join(', ');
      case 'line1': return lines[0] || '';
      case 'line2': return lines[1] || '';
      case 'line3': return lines.slice(2).join(', ');
      case 'city': return V.city;
      case 'region': return V.region;
      case 'postal': return V.postal;
      case 'tel': return V.tel;
      case 'email': return V.email;
    }
    return '';
  };

  var countryNames = function () {
    var names = [V.country];
    var langs = [docProp('documentElement').lang, navigator.language, 'en'];
    langs.forEach(function (lang) {
      try {
        var name = new Intl.DisplayNames([lang], { type: 'region' }).of(V.country);
        if (name && names.indexOf(name) < 0) names.push(name);
      } catch (e) {}
    });
    return names;
  };

  var plain = function (text) {
    return String(text || '').normalize('NFD').replace(/[̀-ͯ]/g, '').trim().toLowerCase();
  };

  var written = window.__wisAddressWritten || (window.__wisAddressWritten = new WeakMap());

  var setValue = function (el, value) {
    var held = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(el), 'value');
    if (held && held.set) held.set.call(el, value); else el.value = value;
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
    written.set(el, { fill: mine, value: el.value });
  };

  var choose = function (select, wanted) {
    var targets = wanted.map(plain).filter(Boolean);
    for (var i = 0; i < select.options.length; i++) {
      var option = select.options[i];
      if (targets.indexOf(plain(option.value)) >= 0 || targets.indexOf(plain(option.text)) >= 0) {
        return option.value;
      }
    }
    return null;
  };

  var fillField = function (field, again) {
    var el = field.el;
    var list = el.tagName === 'SELECT';
    var place = field.kind === 'country' || field.kind === 'region';
    if (!(list && place) && !shownField(el, anchor)) return;
    var mark = written.get(el);
    var earlier = mark && mark.fill !== mine && mark.value === el.value;
    if ((again || el !== anchor) && !earlier && !(list ? untouched(el) : !el.value)) return;
    if (list) {
      var wanted = field.kind === 'country'
        ? (V.country ? countryNames() : [])
        : [valueFor(field.kind, el)];
      var picked = choose(el, wanted);
      if (picked !== null) setValue(el, picked);
      return;
    }
    var text = field.kind === 'country'
      ? (el.maxLength > 0 && el.maxLength <= 3 ? V.country : (V.country ? countryNames()[1] || V.country : ''))
      : valueFor(field.kind, el);
    if (!text || (el.maxLength > 0 && text.length > el.maxLength)) return;
    setValue(el, text);
  };

  var pass = function (again) {
    (addressSection(anchor) || []).forEach(function (field) {
      if (field.kind === 'country') fillField(field, again);
    });
    if (!anchor.isConnected) return;
    (addressSection(anchor) || []).forEach(function (field) {
      if (field.kind !== 'country') fillField(field, again);
    });
  };

  var mine = window.__wisAddressFill = (window.__wisAddressFill || 0) + 1;
  pass(false);
  if (!anchor.isConnected || typeof MutationObserver !== 'function') return;

  var group = groupOf(anchor);
  var target = group === document ? docProp('documentElement') : group;
  var watching = { childList: true, subtree: true, attributes: true };
  var quiet = null;
  var changed = false;
  var over = false;
  var watcher = null;
  var stop = function () {
    over = true;
    watcher.disconnect();
    clearTimeout(quiet);
    clearTimeout(cap);
  };
  var lull = function () {
    clearTimeout(quiet);
    if (over || !changed) return;
    changed = false;
    if (window.__wisAddressFill !== mine || location.origin !== ORIGIN || !anchor.isConnected) {
      stop();
      return;
    }
    watcher.disconnect();
    try {
      pass(true);
    } catch (e) {}
    watcher.observe(target, watching);
  };
  watcher = new MutationObserver(function () {
    changed = true;
    clearTimeout(quiet);
    quiet = setTimeout(lull, 200);
  });
  watcher.observe(target, watching);
  var cap = setTimeout(function () {
    lull();
    stop();
  }, 2000);
} catch (e) {}
"#;

/// Reports focus on an address field, and where it moves, to `wisAddress`.
/// Placed after [`CLASSIFY_JS`] in the page script.
pub(super) const FOCUS_JS: &str = r#"
var anchored = null;
var offered = '';
var token = null;
var settle = null;
var later = null;
var lastReport = -Infinity;
var REPORT_GAP = 300;

function send(body) {
  try {
    var handlers = window.webkit && window.webkit.messageHandlers;
    if (handlers && handlers.wisAddress) {
      handlers.wisAddress.postMessage(JSON.stringify(body));
    }
  } catch (e) {}
}

function measured(el, kind) {
  var r = el.getBoundingClientRect();
  return {
    kind: kind,
    x: r.left,
    y: r.top,
    width: r.width,
    height: r.height,
    viewWidth: window.innerWidth
  };
}

function freshToken() {
  var bytes = window.crypto.getRandomValues(new Uint8Array(16));
  var hex = '';
  for (var i = 0; i < bytes.length; i++) hex += (bytes[i] < 16 ? '0' : '') + bytes[i].toString(16);
  return hex;
}

function anchor(el, kinds) {
  var key = kinds ? kinds.join(' ') : '';
  if (el !== anchored || key !== offered) token = el ? freshToken() : null;
  anchored = el;
  offered = key;
  window.__wisAddressAnchor = el;
  window.__wisAddressToken = token;
}

function quiet() {
  anchor(null);
  clearTimeout(settle);
  settle = null;
  clearTimeout(later);
  later = null;
}

function leave() {
  quiet();
  send({ kind: 'leave' });
}

function report(el) {
  clearTimeout(later);
  later = null;
  if (el !== docProp('activeElement')) return;
  var wait = lastReport + REPORT_GAP - performance.now();
  if (wait > 0) {
    later = setTimeout(function () {
      later = null;
      report(el);
    }, wait);
    return;
  }
  var kinds = offerKinds(el);
  if (!kinds) return;
  lastReport = performance.now();
  anchor(el, kinds);
  var body = measured(el, 'focus');
  body.kinds = kinds;
  body.token = token;
  send(body);
}

document.addEventListener('focusin', function (e) {
  if (window.top !== window.self) return;
  if (anchored && e.target !== anchored) leave();
  if (e.isTrusted) report(e.target);
}, true);

document.addEventListener('pointerdown', function (e) {
  if (anchored && e.target !== anchored) leave();
  if (!e.isTrusted || window.top !== window.self) return;
  if (addressField(e.target) || guessable(e.target)) report(e.target);
}, true);
document.addEventListener('keydown', function (e) {
  if (anchored && e.target === anchored && e.key === 'Escape') leave();
}, true);
document.addEventListener('input', function (e) {
  if (!e.isTrusted) return;
  clearTimeout(later);
  later = null;
  if (anchored && e.target === anchored) leave();
}, true);

function moved(e) {
  if (!anchored) return;
  var scroller = e && e.target;
  if (scroller && scroller !== document && scroller.contains && !scroller.contains(anchored)) return;
  if (settle === null) send({ kind: 'scroll' });
  else clearTimeout(settle);
  settle = setTimeout(function () {
    settle = null;
    if (!anchored) return;
    if (!anchored.isConnected) {
      leave();
      return;
    }
    send(measured(anchored, 'move'));
  }, 150);
}

window.addEventListener('scroll', moved, { capture: true, passive: true });
window.addEventListener('resize', moved, { passive: true });
window.__wisAddressRelease = quiet;
"#;

pub(super) const RELEASE_JS: &str = "window.__wisAddressRelease && window.__wisAddressRelease();";
