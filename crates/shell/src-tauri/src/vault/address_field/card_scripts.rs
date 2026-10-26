//! The scripts `address_field` places in pages for payment cards: which
//! fields take a card, and filling a saved one. The security code is never
//! filled.

/// Classifies card fields by their `cc-*` autocomplete tokens. Placed after
/// [`super::scripts::CLASSIFY_JS`], whose helpers it uses. Defines
/// `cardField`, `cardSection` and `cardOfferKinds`.
pub(super) const CARD_JS: &str = r#"
var CARD_KINDS = {
  'cc-number': 'number',
  'cc-name': 'name',
  'cc-given-name': 'given',
  'cc-family-name': 'family',
  'cc-exp': 'exp',
  'cc-exp-month': 'month',
  'cc-exp-year': 'year',
  'cc-type': 'type'
};

function cardField(el) {
  if (!controlType(el)) return null;
  var tokens = tokensOf(el);
  var kind = own(CARD_KINDS, tokens.pop() || '');
  if (!kind) return null;
  var section = tokens.filter(function (token) {
    return token.indexOf('section-') === 0;
  });
  return { kind: kind, section: section.join(' ') };
}

// The card fields in `el`'s group and section.
function cardSection(el) {
  var mine = cardField(el);
  if (!mine) return null;
  var root = groupOf(el);
  var all = (root.tagName === 'FORM' && read('HTMLFormElement', 'elements', root)) ||
    query(root, 'input, select, textarea');
  var found = [];
  for (var i = 0; i < all.length && i < MAX_FIELDS; i++) {
    if (groupOf(all[i]) !== root) continue;
    var field = cardField(all[i]);
    if (field && field.section === mine.section) found.push({ el: all[i], kind: field.kind });
  }
  return found;
}

// The kinds of card field a fill from `el` would reach, or null unless `el`
// is a shown card field in a section with a shown number field.
function cardOfferKinds(el) {
  if (!cardField(el) || !shownField(el, el)) return null;
  var kinds = [];
  (cardSection(el) || []).forEach(function (field) {
    var shown = field.el.tagName === 'SELECT' || shownField(field.el, el);
    if (shown && kinds.indexOf(field.kind) < 0) kinds.push(field.kind);
  });
  return kinds.indexOf('number') >= 0 ? kinds : null;
}
"#;

/// Fills the card section of the field the card was shown for with the
/// values `V` (`number`, `name`, `month`, `year`, and `brand`, the names a
/// card type list may use), once the page is still on `ORIGIN`, in that field
/// and on the report `TOKEN` came with. Fields already holding a value are
/// left alone, apart from the focused one.
pub(super) const CARD_FILL_JS: &str = r#"
try {
  if (location.origin !== ORIGIN || window.__wisAddressToken !== TOKEN) return;
  var anchor = window.__wisAddressAnchor;
  if (!anchor || !anchor.isConnected || anchor !== docProp('activeElement')) return;

  var two = function (n) { return (n < 10 ? '0' : '') + n; };
  var words = String(V.name || '').trim().split(/\s+/).filter(Boolean);
  var plain = function (text) { return String(text || '').trim().toLowerCase(); };
  var leading = function (text) { return parseInt(String(text || '').trim(), 10); };

  var setValue = function (el, value) {
    var held = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(el), 'value');
    if (held && held.set) held.set.call(el, value); else el.value = value;
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  };

  var pick = function (select, matches) {
    for (var i = 0; i < select.options.length; i++) {
      var option = select.options[i];
      if (option.value !== '' && matches(option)) return option.value;
    }
    return null;
  };

  var hintOf = function (el) {
    return String(el.getAttribute('placeholder') || '') + ' ' + String(el.getAttribute('aria-label') || '');
  };
  var longYear = function (hint) { return /yyyy|aaaa|jjjj/i.test(hint); };
  var shortYear = function (hint) { return !longYear(hint) && /\b(yy|aa|jj)\b/i.test(hint); };

  var expiry = function (el) {
    var hint = hintOf(el);
    var full = String(V.year);
    var years = longYear(hint) ? [full] : shortYear(hint) ? [two(V.year % 100)] : [two(V.year % 100), full];
    var gaps = /\s\/\s/.test(hint) ? [' / ', '/', ''] : ['/', ' / ', ''];
    var shapes = [];
    years.forEach(function (year) {
      gaps.forEach(function (gap) { shapes.push(two(V.month) + gap + year); });
    });
    for (var i = 0; i < shapes.length; i++) {
      if (!(el.maxLength > 0) || shapes[i].length <= el.maxLength) return shapes[i];
    }
    return '';
  };

  var listed = function (el, kind) {
    switch (kind) {
      case 'month':
        return pick(el, function (o) { return leading(o.value) === V.month || leading(o.text) === V.month; });
      case 'year':
        return pick(el, function (o) {
          var n = leading(o.value) || leading(o.text);
          return n === V.year || n === V.year % 100;
        });
      case 'type':
        var names = (V.brand || []).map(plain);
        return pick(el, function (o) { return names.indexOf(plain(o.text)) >= 0 || names.indexOf(plain(o.value)) >= 0; });
    }
    return null;
  };

  var typed = function (el, kind) {
    switch (kind) {
      case 'number': return V.number;
      case 'name': return words.join(' ');
      case 'given': return words.slice(0, -1).join(' ') || words.join(' ');
      case 'family': return words.length > 1 ? words[words.length - 1] : '';
      case 'exp': return V.month ? expiry(el) : '';
      case 'month': return V.month ? two(V.month) : '';
      case 'year':
        if (!V.year) return '';
        return el.maxLength === 2 || shortYear(hintOf(el)) ? two(V.year % 100) : String(V.year);
    }
    return '';
  };

  (cardSection(anchor) || []).forEach(function (field) {
    var el = field.el;
    var list = el.tagName === 'SELECT';
    if (!list && !shownField(el, anchor)) return;
    if (el !== anchor && (list ? !untouched(el) : el.value)) return;
    if (list) {
      if ((field.kind === 'month' || field.kind === 'year') && !V.month) return;
      var picked = listed(el, field.kind);
      if (picked !== null) setValue(el, picked);
      return;
    }
    var text = typed(el, field.kind);
    if (!text || (el.maxLength > 0 && text.length > el.maxLength)) return;
    setValue(el, text);
  });
} catch (e) {}
"#;
