//! Reporting an address or card a person typed when its form is submitted,
//! for address (`address_save`) and card (`card_save`) saving.

/// Reports `{kind: 'save', values}` for an address, and `{kind: 'card',
/// values}` for a card whose number the person typed, to `wisAddress` on a
/// real submit, a click on a button that submits, or Enter in an address or
/// card field. Never the security code. Placed after `FOCUS_JS`, whose `send`
/// it uses.
pub(super) const SUBMIT_JS: &str = r#"
var COUNTRY_CODES = (
  'AD AE AF AG AI AL AM AO AQ AR AS AT AU AW AX AZ BA BB BD BE BF BG BH BI BJ BL BM BN BO BQ BR ' +
  'BS BT BV BW BY BZ CA CC CD CF CG CH CI CK CL CM CN CO CR CU CV CW CX CY CZ DE DJ DK DM DO DZ ' +
  'EC EE EG EH ER ES ET FI FJ FK FM FO FR GA GB GD GE GF GG GH GI GL GM GN GP GQ GR GS GT GU GW ' +
  'GY HK HM HN HR HT HU ID IE IL IM IN IO IQ IR IS IT JE JM JO JP KE KG KH KI KM KN KP KR KW KY ' +
  'KZ LA LB LC LI LK LR LS LT LU LV LY MA MC MD ME MF MG MH MK ML MM MN MO MP MQ MR MS MT MU MV ' +
  'MW MX MY MZ NA NC NE NF NG NI NL NO NP NR NU NZ OM PA PE PF PG PH PK PL PM PN PR PS PT PW PY ' +
  'QA RE RO RS RU RW SA SB SC SD SE SG SH SI SJ SK SL SM SN SO SR SS ST SV SX SY SZ TC TD TF TG ' +
  'TH TJ TK TL TM TN TO TR TT TV TW TZ UA UG UM US UY UZ VA VC VE VG VI VN VU WF WS YE YT ZA ZM ZW'
).split(' ');
var SUBMIT_WORDS = /\b(log\s?in|sign\s?in|sign\s?up|register|continue|submit|next|done|confirm|save|order|pay|check\s?out|buy|purchase|proceed|ship|deliver|review)\b/;
var GESTURE_MS = 1000;
var lastSave = '';
var lastCard = '';
var lastGesture = -Infinity;
var typed = new WeakSet();

function folded(text) {
  return String(text || '').normalize('NFD').replace(/[̀-ͯ]/g, '').trim().toLowerCase();
}

function countryCode(text) {
  var wanted = folded(text);
  if (!wanted) return '';
  if (/^[a-z]{2}$/.test(wanted)) {
    return COUNTRY_CODES.indexOf(wanted.toUpperCase()) >= 0 ? wanted.toUpperCase() : '';
  }
  var langs = [docProp('documentElement').lang, navigator.language, 'en'];
  for (var l = 0; l < langs.length; l++) {
    var names = null;
    try {
      names = new Intl.DisplayNames([langs[l]], { type: 'region' });
    } catch (e) {
      continue;
    }
    for (var i = 0; i < COUNTRY_CODES.length; i++) {
      if (folded(names.of(COUNTRY_CODES[i])) === wanted) return COUNTRY_CODES[i];
    }
  }
  return '';
}

// The values a person typed into one section, or null when it holds no
// street with a city or postal code. Only shown fields count, and the lists
// of countries or regions a page styles away.
function typedValues(fields) {
  var values = {};
  var lines = [];
  var typedStreet = false;
  var typedPlace = false;
  var changed = false;
  var written = window.__wisAddressWritten;
  fields.forEach(function (field) {
    var el = field.el;
    var list = el.tagName === 'SELECT';
    var place = field.kind === 'country' || field.kind === 'region';
    if (!(list && place) && !shownField(el, null)) return;
    var raw = String(el.value || '').trim();
    if (!raw) return;
    var text = raw;
    if (list) {
      var option = el.options[el.selectedIndex];
      text = option ? String(option.text || '').trim() : '';
    }
    if (field.kind === 'country') text = countryCode(raw) || countryCode(text);
    if (field.kind === 'region' && list) values.regionCode = raw;
    if (!text) return;
    var line = field.kind === 'street' || field.kind.indexOf('line') === 0;
    var mark = written && written.get(el);
    if (!mark || mark.value !== el.value) changed = true;
    if (typed.has(el)) {
      if (line) typedStreet = true;
      if (field.kind === 'city' || field.kind === 'postal') typedPlace = true;
    }
    if (line) lines.push(text);
    else if (!own(values, field.kind)) values[field.kind] = text;
  });
  if (lines.length) values.street = lines.join('\n');
  return typedStreet && typedPlace && changed ? values : null;
}

// The first section of `root` that holds a typed address.
function typedAddress(root) {
  var sections = [];
  var bySection = {};
  addressFields(root).forEach(function (field) {
    if (!own(bySection, field.section)) {
      bySection[field.section] = [];
      sections.push(field.section);
    }
    bySection[field.section].push(field);
  });
  for (var i = 0; i < sections.length; i++) {
    var values = typedValues(bySection[sections[i]]);
    if (values) return values;
  }
  return null;
}

// The card in the first section of `root` whose number the user typed,
// with its month and year read from one expiry box or two, or null.
function typedCard(root) {
  var bySection = {};
  var order = [];
  cardFields(root).forEach(function (field) {
    if (!own(bySection, field.section)) {
      bySection[field.section] = [];
      order.push(field.section);
    }
    bySection[field.section].push(field);
  });
  for (var i = 0; i < order.length; i++) {
    var card = { number: '', name: '', month: 0, year: 0 };
    var given = '';
    var family = '';
    var typedNumber = false;
    bySection[order[i]].forEach(function (field) {
      var el = field.el;
      var list = el.tagName === 'SELECT';
      if (!list && !shownField(el, null)) return;
      var raw = String(el.value || '').trim();
      if (!raw) return;
      var number = parseInt(raw, 10) || parseInt(list && el.options[el.selectedIndex] ?
        el.options[el.selectedIndex].text : '', 10) || 0;
      switch (field.kind) {
        case 'number':
          card.number = raw;
          typedNumber = typed.has(el);
          break;
        case 'name': card.name = raw; break;
        case 'given': given = raw; break;
        case 'family': family = raw; break;
        case 'month': card.month = number; break;
        case 'year': card.year = number; break;
        case 'exp':
          var parts = /^(\d{1,2})\s*[\/\-.]?\s*(\d{2}|\d{4})$/.exec(raw);
          if (parts) {
            card.month = parseInt(parts[1], 10);
            card.year = parseInt(parts[2], 10);
          }
          break;
      }
    });
    if (!card.name) card.name = [given, family].filter(Boolean).join(' ');
    if (typedNumber && card.month && card.year) return card;
  }
  return null;
}

function offerToSave(root) {
  if (!root || window.top !== window.self) return;
  var card = typedCard(root);
  if (card) {
    var cardKey = [card.number.slice(-4), card.month, card.year, card.name].join(' ');
    if (cardKey !== lastCard) {
      lastCard = cardKey;
      send({ kind: 'card', values: card });
    }
  }
  var values = typedAddress(root);
  if (!values) return;
  var key = JSON.stringify(values);
  if (key === lastSave) return;
  lastSave = key;
  send({ kind: 'save', values: values });
}

function submitter(target) {
  for (var node = target, hops = 0; node && node.nodeType === 1 && hops < 5;
      node = parentOf(node), hops++) {
    var tag = node.tagName;
    var type = String(node.getAttribute('type') || '').toLowerCase();
    var role = String(node.getAttribute('role') || '').toLowerCase();
    if (tag === 'INPUT') return type === 'submit' || type === 'image' ? node : null;
    if (tag !== 'BUTTON' && tag !== 'A' && role !== 'button') continue;
    if (tag === 'BUTTON' && (type === 'submit' || (!type && node.form))) return node;
    var words = textIn(node, false) + ' ' + (node.getAttribute('aria-label') || '') + ' ' +
      (node.value || '');
    return SUBMIT_WORDS.test(words.toLowerCase()) ? node : null;
  }
  return null;
}

function inner(e) {
  return (e.composedPath && e.composedPath()[0]) || e.target;
}

document.addEventListener('input', function (e) {
  if (e.isTrusted) typed.add(inner(e));
}, true);
document.addEventListener('change', function (e) {
  var el = inner(e);
  if (e.isTrusted && el && el.tagName === 'SELECT') typed.add(el);
}, true);
var gesture = function (e) {
  if (e.isTrusted) lastGesture = performance.now();
};
document.addEventListener('pointerdown', gesture, true);
document.addEventListener('keydown', gesture, true);

document.addEventListener('submit', function (e) {
  if (e.isTrusted && performance.now() - lastGesture < GESTURE_MS) offerToSave(e.target);
}, true);
document.addEventListener('click', function (e) {
  if (!e.isTrusted) return;
  var button = submitter(e.target);
  if (button) offerToSave(groupOf(button));
}, true);
document.addEventListener('keydown', function (e) {
  if (!e.isTrusted || e.key !== 'Enter' || e.defaultPrevented) return;
  var el = e.target;
  if (!el || el.tagName === 'TEXTAREA') return;
  if (addressField(el) || cardField(el) || guessable(el)) offerToSave(groupOf(el));
});
"#;
