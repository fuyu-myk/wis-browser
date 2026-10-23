//! Reading address fields that carry no `autocomplete` token from the words
//! around them, with Chromium's patterns (`patterns.js`).

/// Defines `guessKinds` and `maybeAddress` for `CLASSIFY_JS`. Placed after
/// `patterns.js` and before `CLASSIFY_JS`, whose helpers it uses.
pub(super) const GUESS_JS: &str = r#"
// Chromium's order. A null kind marks a field that is known and left alone.
var GUESS_ORDER = [
  ['SEARCH_TERM', null], ['ONE_TIME_CODE', null], ['EMAIL_ADDRESS', 'email'], ['PHONE', 'tel'],
  ['ADDRESS_LOOKUP', null], ['ATTENTION_IGNORED', null], ['ADDRESS_NAME_IGNORED', null],
  ['ADDRESS_LINE_1', 'line1'], ['PHONE_COUNTRY_CODE', null], ['ZIP_4', null],
  ['ZIP_CODE', 'postal'], ['COUNTRY', 'country'], ['STATE', 'region'], ['CITY', 'city'],
  ['COMPANY_NAME', 'organization'], ['NAME_ON_CARD', null], ['NAME_IGNORED', null],
  ['HONORIFIC_PREFIX', null], ['MIDDLE_INITIAL', null], ['MIDDLE_NAME', null],
  ['LAST_NAME_SECOND', null], ['LAST_NAME_FIRST', 'family'], ['FIRST_NAME', 'given'],
  ['LAST_NAME', 'family'], ['FULL_NAME', 'name']
];
var PHONE_PARTS = ['PHONE_AREA_CODE', 'PHONE_PREFIX', 'PHONE_SUFFIX'];
var INLINE = ['B', 'STRONG', 'SPAN', 'FONT'];
var SKIP_TEXT = ['INPUT', 'SELECT', 'TEXTAREA', 'OPTION', 'BUTTON', 'SCRIPT', 'STYLE'];
var WORDS = 200;
var guessFor = null;

// The text inside `node`, leaving out form controls and their options, or
// null when `strict` and it holds a control.
function textIn(node, strict) {
  var out = '';
  var visits = 0;
  var control = false;
  var walk = function (n) {
    if (control || out.length > WORDS || ++visits > 200 || !n) return;
    if (n.nodeType === 3) {
      out += n.nodeValue;
      return;
    }
    if (n.nodeType !== 1) return;
    if (SKIP_TEXT.indexOf(n.tagName) >= 0) {
      control = n.tagName !== 'SCRIPT' && n.tagName !== 'STYLE';
      return;
    }
    var kids = read('Node', 'childNodes', n) || [];
    for (var i = 0; i < kids.length; i++) walk(kids[i]);
  };
  walk(node);
  if (strict && control) return null;
  return out.replace(/\s+/g, ' ').trim().slice(0, WORDS);
}

function byId(id) {
  try {
    return Document.prototype.getElementById.call(document, id);
  } catch (e) {
    return document.getElementById(id);
  }
}

// The words just before `node` as Chromium reads them: text and inline
// elements run together, a `p` or `label` stands alone, and any other
// element, such as a heading, ends them.
function siblingWords(node) {
  var words = [];
  var before = read('Node', 'previousSibling', node);
  for (var n = 0; before && n < 10; n++, before = read('Node', 'previousSibling', before)) {
    if (before.nodeType === 3) {
      var said = String(before.nodeValue).trim();
      if (said) words.unshift(said);
      continue;
    }
    if (before.nodeType !== 1 || before.tagName === 'IMG' || before.tagName === 'BR') continue;
    var inline = INLINE.indexOf(before.tagName) >= 0;
    if (!inline && (words.length || (before.tagName !== 'P' && before.tagName !== 'LABEL'))) break;
    var inside = textIn(before, true);
    if (inside === null) break;
    if (inside) words.unshift(inside);
    if (!inline) break;
  }
  return words.join(' ').slice(0, WORDS);
}

// The words in the cell before the one `el` sits in, for a form laid out as a
// table.
function cellWords(el) {
  for (var node = parentOf(el), hops = 0; node && hops < 3; node = parentOf(node), hops++) {
    if (node.tagName !== 'TD' && node.tagName !== 'TH') continue;
    var before = read('Node', 'previousSibling', node);
    while (before && before.nodeType !== 1) before = read('Node', 'previousSibling', before);
    return before && (before.tagName === 'TD' || before.tagName === 'TH')
      ? textIn(before, true) || ''
      : '';
  }
  return '';
}

// Words just before `el`, or, when it has no placeholder of its own, before
// its parent or in the table cell before its own, for forms that name a
// field without a label.
function nearbyWords(el) {
  var words = siblingWords(el);
  if (words || el.getAttribute('placeholder')) return words;
  var up = parentOf(el);
  return (up ? siblingWords(up) : '') || cellWords(el);
}

function labelWords(el) {
  var parts = [];
  var labels = el.labels || [];
  for (var i = 0; i < labels.length && i < 3; i++) parts.push(textIn(labels[i], false));
  String(el.getAttribute('aria-labelledby') || '').split(/\s+/).filter(Boolean).slice(0, 3)
    .forEach(function (id) {
      var node = byId(id);
      if (node) parts.push(textIn(node, false));
    });
  parts.push(el.getAttribute('aria-label') || '');
  var words = parts.join(' ').replace(/\s+/g, ' ').trim().slice(0, WORDS);
  return words || nearbyWords(el);
}

function describe(el) {
  var label = labelWords(el);
  var placeholder = String(el.getAttribute('placeholder') || '').slice(0, WORDS);
  return {
    type: controlType(el),
    label: label,
    placeholder: placeholder,
    name: String(el.getAttribute('name') || el.getAttribute('id') || '').slice(0, WORDS),
    short: el.maxLength > 0 && el.maxLength <= 4,
    bare: !/[\p{L}\p{N}]/u.test(label + placeholder)
  };
}

// The patterns for the page's language with Chromium's English ones for
// markup names, or every language's when the page's has none.
function patternSet() {
  var lang = String(docProp('documentElement').lang || '').toLowerCase();
  if (guessFor && guessFor.lang === lang) return guessFor.keys;
  var all = addressPatterns();
  var langs = {};
  Object.keys(all).forEach(function (key) {
    all[key].forEach(function (entry) {
      langs[entry[0].toLowerCase()] = entry[0];
    });
  });
  var picked = own(langs, lang) || own(langs, lang.split('-')[0]) || '';
  var keys = {};
  Object.keys(all).forEach(function (key) {
    keys[key] = all[key].filter(function (entry) {
      return !picked || entry[0] === picked || entry[0] === 'en';
    }).map(function (entry) {
      var english = picked && picked !== 'en' && entry[0] === 'en';
      return { pos: entry[1], neg: entry[2], attrs: english ? 2 : entry[3], types: entry[4] };
    });
  });
  guessFor = { lang: lang, keys: keys };
  return keys;
}

function compiled(entry) {
  if (entry.re === undefined) {
    try {
      entry.re = new RegExp(entry.pos, 'i');
      entry.nre = entry.neg ? new RegExp(entry.neg, 'i') : null;
    } catch (e) {
      entry.re = null;
    }
  }
  return entry.re;
}

// Whether a pattern of `key` names `field`: its positive pattern matches one
// of the words it reads, and its negative pattern none.
function matches(key, field) {
  var entries = own(patternSet(), key) || [];
  for (var i = 0; i < entries.length; i++) {
    var entry = entries[i];
    if (!(entry.types & field.type) || !compiled(entry)) continue;
    var texts = [];
    if (entry.attrs & 1) texts.push(field.label, field.placeholder);
    if (entry.attrs & 2) texts.push(field.name);
    var hit = false;
    var vetoed = false;
    texts.forEach(function (text) {
      if (!text) return;
      if (entry.nre && entry.nre.test(text)) vetoed = true;
      else if (entry.re.test(text)) hit = true;
    });
    if (hit && !vetoed) return true;
  }
  return false;
}

function guessOne(field, carded) {
  for (var i = 0; i < GUESS_ORDER.length; i++) {
    var key = GUESS_ORDER[i][0];
    var kind = GUESS_ORDER[i][1];
    if (key === 'NAME_ON_CARD' && !carded) continue;
    if (!matches(key, field)) continue;
    if (kind === 'tel' && (matches('PHONE_EXTENSION', field) || (field.short &&
        PHONE_PARTS.some(function (part) { return matches(part, field); })))) {
      return null;
    }
    return kind;
  }
  return null;
}

function maybeAddress(el) {
  var field = describe(el);
  return GUESS_ORDER.some(function (step) {
    return step[1] && matches(step[0], field);
  }) || matches('ADDRESS_LINE_2', field);
}

// Whether `field`, right after a postal code, is its ZIP+4 box.
function zipTail(field) {
  return matches('ZIP_4', field) || (field.short && (field.bare || matches('ZIP_CODE', field)));
}

// The kinds of `controls`, in page order. A phone split over adjacent boxes,
// one short, unlabelled or named as a part, and a ZIP+4 box are left alone.
// A first or last name counts only beside the other, else as a full name.
function guessKinds(controls, carded) {
  var fields = controls.map(describe);
  carded = carded || fields.some(function (field) { return matches('CREDIT_CARD_NUMBER', field); });
  var out = [];
  var seen = {};
  var section = 1;
  var last = '';
  fields.forEach(function (field, i) {
    if (last === 'postal' && zipTail(field)) {
      last = 'zip4';
      return;
    }
    var kind;
    if (last === 'line1' && matches('ADDRESS_LINE_2', field)) kind = 'line2';
    else if (last === 'line2' && matches('ADDRESS_LINE_EXTRA', field)) kind = 'line3';
    else kind = guessOne(field, carded);
    var again = kind === last && (kind === 'email' || kind === 'tel');
    last = kind || '';
    if (!kind) return;
    if (own(seen, kind) && !again) {
      section += 1;
      seen = {};
    }
    seen[kind] = true;
    out.push({ el: controls[i], kind: kind, section: 'guess-' + section, guessed: true, at: i });
  });
  var piece = function (field) {
    return field.short || field.bare || PHONE_PARTS.some(function (part) {
      return matches(part, field);
    });
  };
  var named = {};
  var phones = {};
  var split = {};
  out.forEach(function (found) {
    named[found.section + ' ' + found.kind] = true;
    if (found.kind !== 'tel') return;
    if (phones[found.at - 1] && (piece(fields[found.at - 1]) || piece(fields[found.at]))) {
      split[found.at - 1] = true;
      split[found.at] = true;
    }
    phones[found.at] = true;
  });
  out = out.filter(function (found) {
    return !split[found.at];
  });
  return out.map(function (found) {
    var other = found.kind === 'given' ? 'family' : found.kind === 'family' ? 'given' : '';
    var kind = other && !named[found.section + ' ' + other]
      ? (matches('FULL_NAME', fields[found.at]) ? 'name' : null)
      : found.kind;
    return { el: found.el, kind: kind, section: found.section, guessed: true };
  }).filter(function (found) {
    return found.kind;
  });
}
"#;
