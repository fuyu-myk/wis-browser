// Which fields `vault/address_field` takes for a payment card, and how a card
// fill shapes the number, name and expiry for each of them.

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { includedFile, rawStringConst } from '../support/rustSource.mjs'

const CLASSIFY = [
  includedFile('vault/address_field/patterns.js'),
  rawStringConst('vault/address_field/guess.rs', 'GUESS_JS'),
  rawStringConst('vault/address_field/scripts.rs', 'CLASSIFY_JS'),
  rawStringConst('vault/address_field/card_scripts.rs', 'CARD_JS'),
].join('\n')
const FILL = rawStringConst('vault/address_field/card_scripts.rs', 'CARD_FILL_JS')
const FOCUS = rawStringConst('vault/address_field/scripts.rs', 'FOCUS_JS')

const SHOWN_STYLE = {
  display: 'block',
  visibility: 'visible',
  opacity: '1',
  fontSize: '13px',
  color: 'rgb(0, 0, 0)',
  webkitTextFillColor: 'rgb(0, 0, 0)',
  clipPath: 'none',
  clip: 'auto',
  contain: 'none',
  mixBlendMode: 'normal',
  position: 'static',
  maskImage: 'none',
  filter: 'none',
  overflow: 'visible',
  overflowX: 'visible',
  overflowY: 'visible',
}

let row = 0

/** A form control with what the scripts read, each on its own row. */
function field(autocomplete, spec = {}) {
  const top = 10 + 40 * row++
  const el = {
    nodeType: 1,
    tagName: 'INPUT',
    disabled: false,
    readOnly: false,
    isConnected: true,
    value: '',
    maxLength: -1,
    form: null,
    parentElement: null,
    box: { left: 10, top, right: 210, bottom: top + 30, width: 200, height: 30 },
    ...spec,
    attrs: { autocomplete, ...(spec.attrs || {}) },
    style: { ...SHOWN_STYLE, ...(spec.style || {}) },
  }
  el.getAttribute = (name) => (name in el.attrs ? el.attrs[name] : null)
  el.getBoundingClientRect = () => el.box
  el.contains = (other) => other === el
  el.dispatchEvent = () => true
  if (el.tagName === 'SELECT') {
    el.options ||= []
    el.selectedIndex = 0
    Object.defineProperty(el, 'value', {
      get: () => el.options[el.selectedIndex]?.value ?? '',
      set: (value) => {
        el.selectedIndex = el.options.findIndex((option) => option.value === value)
      },
    })
  }
  return el
}

const select = (autocomplete, options) =>
  field(autocomplete, {
    tagName: 'SELECT',
    options: options.map(([value, text]) => ({ value, text, defaultSelected: false })),
  })

function page(controls, { anchor = controls[0], origin = 'https://shop.example', token = 'offer-1' } = {}) {
  const at = (x, y) => (el) => x >= el.box.left && x < el.box.right && y >= el.box.top && y < el.box.bottom
  return {
    doc: {
      activeElement: anchor,
      documentElement: {
        lang: 'en',
        scrollWidth: 1200,
        scrollHeight: 5000,
        style: { overflowX: 'visible', overflowY: 'visible' },
      },
      querySelectorAll: () => controls,
      elementFromPoint: (x, y) => controls.find(at(x, y)) || null,
    },
    location: { origin },
    window: {
      scrollX: 0,
      scrollY: 0,
      innerWidth: 1280,
      innerHeight: 800,
      __wisAddressAnchor: anchor,
      __wisAddressToken: token,
    },
    getComputedStyle: (el) => el.style,
  }
}

function classify(env) {
  return new Function(
    'document',
    'window',
    'getComputedStyle',
    `${CLASSIFY}\nreturn { cardField, cardOfferKinds, offerKinds }`,
  )(env.doc, env.window, env.getComputedStyle)
}

const visa = { number: '4242424242424242', name: 'Ada Lovelace', month: 8, year: 2028, brand: ['visa', 'Visa'] }

function fill(env, values = visa, origin = 'https://shop.example', token = 'offer-1') {
  new Function(
    'document',
    'location',
    'window',
    'getComputedStyle',
    'navigator',
    'Event',
    'ORIGIN',
    'TOKEN',
    'V',
    `${CLASSIFY}\n${FILL}`,
  )(
    env.doc,
    env.location,
    env.window,
    env.getComputedStyle,
    { language: 'en' },
    class {
      constructor(type) {
        this.type = type
      }
    },
    origin,
    token,
    values,
  )
}

test('a card field is known by its cc token, the security code never', () => {
  const { cardField } = classify(page([]))
  assert.deepEqual(cardField(field('cc-number')), { kind: 'number', section: '' })
  assert.deepEqual(cardField(field('section-pay billing cc-exp')), { kind: 'exp', section: 'section-pay' })
  assert.deepEqual(cardField(field('cc-exp-month', { tagName: 'SELECT' })), { kind: 'month', section: '' })
  for (const el of [field('cc-csc'), field('postal-code'), field('cc-number', { disabled: true }), field('off')]) {
    assert.equal(cardField(el), null)
  }
})

test('a card is offered at shown card fields beside a number, and never as an address', () => {
  const number = field('cc-number')
  const name = field('cc-name')
  const exp = field('cc-exp')
  const csc = field('cc-csc')
  const hidden = field('cc-family-name', { style: { display: 'none' } })
  const env = page([number, name, exp, csc, hidden])
  const { cardOfferKinds, offerKinds } = classify(env)
  assert.deepEqual(cardOfferKinds(name), ['number', 'name', 'exp'])
  assert.equal(offerKinds(name), null)
  assert.equal(cardOfferKinds(csc), null)

  const alone = field('cc-name')
  assert.equal(classify(page([alone])).cardOfferKinds(alone), null, 'no number field')
})

test('the number, name and a one-box expiry are filled, shaped to each box', () => {
  const number = field('cc-number')
  const name = field('cc-name')
  const short = field('cc-exp', { maxLength: 5 })
  fill(page([number, name, short]))
  assert.deepEqual([number.value, name.value, short.value], ['4242424242424242', 'Ada Lovelace', '08/28'])

  const spaced = field('cc-number')
  const long = field('cc-exp', { attrs: { placeholder: 'MM / YYYY' } })
  const compact = field('section-b cc-exp', { maxLength: 4 })
  fill(page([spaced, long]))
  assert.equal(long.value, '08 / 2028')
  const other = field('section-b cc-number')
  fill(page([other, compact]))
  assert.equal(compact.value, '0828')

  const roomy = field('cc-exp', { maxLength: 7, attrs: { placeholder: 'MM / YY' } })
  fill(page([field('cc-number'), roomy], { anchor: roomy }))
  assert.equal(roomy.value, '08 / 28', 'a two-digit hint keeps a two-digit year')
})

test('a year box takes two digits when its hint or size asks for them', () => {
  const hinted = field('cc-exp-year', { attrs: { placeholder: 'YY' } })
  fill(page([field('cc-number'), hinted], { anchor: hinted }))
  assert.equal(hinted.value, '28')
  const plain = field('cc-exp-year')
  fill(page([field('cc-number'), plain], { anchor: plain }))
  assert.equal(plain.value, '2028')
})

test('split names and expiry lists get their own part', () => {
  const number = field('cc-number')
  const given = field('cc-given-name')
  const family = field('cc-family-name')
  const month = select('cc-exp-month', [['', 'Month'], ['1', '01 - January'], ['8', '08 - August']])
  const year = select('cc-exp-year', [['', 'Year'], ['27', '2027'], ['28', '2028']])
  const kind = select('cc-type', [['', 'Card type'], ['MC', 'Mastercard'], ['VI', 'Visa']])
  fill(page([number, given, family, month, year, kind]))
  assert.deepEqual(
    [given.value, family.value, month.value, year.value, kind.value],
    ['Ada', 'Lovelace', '8', '28', 'VI'],
  )

  const twoDigit = field('cc-exp-year', { maxLength: 2 })
  const monthBox = field('cc-exp-month')
  const again = field('cc-number')
  fill(page([again, monthBox, twoDigit]))
  assert.deepEqual([monthBox.value, twoDigit.value], ['08', '28'])
})

test('only the values sent are filled, the security code and filled fields are left alone', () => {
  const number = field('cc-number')
  const name = field('cc-name', { value: 'A. Lovelace' })
  const csc = field('cc-csc')
  const exp = field('cc-exp')
  fill(page([number, name, csc, exp]), { number: '4242424242424242', name: '', month: 0, year: 0, brand: [] })
  assert.deepEqual([number.value, name.value, csc.value, exp.value], ['4242424242424242', 'A. Lovelace', '', ''])
})

test('nothing is filled on another origin, an older offer, or once the field has gone', () => {
  const make = () => [field('cc-number'), field('cc-name')]
  const moved = make()
  fill(page(moved), visa, 'https://other.example')
  assert.deepEqual(moved.map((el) => el.value), ['', ''])

  const stale = make()
  fill(page(stale, { token: 'offer-2' }))
  assert.deepEqual(stale.map((el) => el.value), ['', ''])

  const gone = make()
  gone[0].isConnected = false
  fill(page(gone))
  assert.deepEqual(gone.map((el) => el.value), ['', ''])
})

test('a focus on a card field is reported as a card offer', () => {
  const sent = []
  const listeners = {}
  const form = {}
  const number = field('cc-number', { form })
  const exp = field('cc-exp', { form })
  const controls = [number, exp]
  form.querySelectorAll = () => controls
  const window = {
    innerWidth: 1280,
    innerHeight: 800,
    scrollX: 0,
    scrollY: 0,
    crypto: { getRandomValues: (bytes) => bytes.fill(7) },
    webkit: { messageHandlers: { wisAddress: { postMessage: (body) => sent.push(JSON.parse(body)) } } },
    addEventListener: () => undefined,
  }
  window.top = window.self = window
  const document = {
    activeElement: number,
    documentElement: { lang: 'en', scrollWidth: 1200, scrollHeight: 5000, style: {} },
    addEventListener: (type, fn) => (listeners[type] ||= []).push(fn),
    querySelectorAll: () => controls,
    elementFromPoint: () => null,
  }
  new Function(
    'window',
    'document',
    'getComputedStyle',
    'setTimeout',
    'clearTimeout',
    'performance',
    `${CLASSIFY}\n${FOCUS}`,
  )(window, document, (el) => el.style, () => 0, () => undefined, { now: () => 0 })
  listeners.focusin.forEach((fn) => fn({ target: number, isTrusted: true }))
  assert.equal(sent.length, 1)
  assert.equal(sent[0].form, 'card')
  assert.deepEqual(sent[0].kinds, ['number', 'exp'])
})

const bare = (attrs, spec = {}) => field(undefined, { ...spec, attrs })

test('untagged card fields are read from their words, the security code kept apart', () => {
  const number = bare({ name: 'cardnumber', placeholder: 'Card number' })
  const exp = bare({ name: 'exp-date', placeholder: 'MM / YY' })
  const cvc = bare({ name: 'cvc', placeholder: 'Card code' })
  const name = bare({ name: 'ccname', placeholder: 'Name on card' })
  const env = page([number, exp, cvc, name])
  const { cardOfferKinds, offerKinds } = classify(env)
  assert.deepEqual(cardOfferKinds(number), ['number', 'exp', 'name'])
  assert.deepEqual(cardOfferKinds(name), ['number', 'exp', 'name'])
  assert.equal(cardOfferKinds(cvc), null)
  assert.equal(offerKinds(name), null, 'not an address name')

  fill(page([number, exp, cvc, name]))
  assert.deepEqual(
    [number.value, exp.value, cvc.value, name.value],
    ['4242424242424242', '08 / 28', '', 'Ada Lovelace'],
  )
})

test('an untagged month list followed by a year list is an expiry pair', () => {
  const number = bare({ name: 'card_number', 'aria-label': 'Card number' })
  const month = select(undefined, [['', 'MM'], ['08', '08']])
  month.attrs = { name: 'exp_month' }
  const year = select(undefined, [['', 'YYYY'], ['2028', '2028']])
  year.attrs = { name: 'exp_year' }
  const env = page([number, month, year])
  assert.deepEqual(classify(env).cardOfferKinds(number), ['number', 'month', 'year'])
  fill(env)
  assert.deepEqual([month.value, year.value], ['08', '2028'])
})

test('a gift card number, or a form with no card number, is not offered a card', () => {
  const gift = bare({ name: 'giftcard', placeholder: 'Gift card number' })
  const pin = bare({ name: 'pin', placeholder: 'PIN' })
  assert.equal(classify(page([gift, pin])).cardOfferKinds(gift), null)

  const postal = bare({ name: 'zip', placeholder: 'ZIP code' })
  const city = bare({ name: 'city', placeholder: 'City' })
  const street = bare({ name: 'address1', placeholder: 'Street address' })
  const { cardOfferKinds, offerKinds } = classify(page([street, city, postal]))
  assert.equal(cardOfferKinds(postal), null)
  assert.ok(offerKinds(postal), 'still an address')
})

const inLang = (env, lang) => {
  env.doc.documentElement.lang = lang
  return env
}

test('house numbers and tax numbers on address forms are not card numbers', () => {
  const street = bare({ name: 'rua', placeholder: 'Rua' })
  const house = bare({ name: 'numero', placeholder: 'Número' })
  const postal = bare({ name: 'cep', placeholder: 'CEP' })
  const city = bare({ name: 'cidade', placeholder: 'Cidade' })
  assert.equal(classify(inLang(page([street, house, postal, city]), 'pt-BR')).cardOfferKinds(house), null)

  const dutch = bare({ name: 'huisnummer', placeholder: 'Huisnummer' })
  const straat = bare({ name: 'straat', placeholder: 'Straat' })
  assert.equal(classify(page([straat, dutch])).cardOfferKinds(dutch), null)

  const tax = bare({ name: 'steuernummer', placeholder: 'Steuernummer' })
  assert.equal(classify(inLang(page([tax]), 'de')).cardOfferKinds(tax), null)
})

test('only fields next to the card number join it, so a birth date or shipping name stays out', () => {
  const name = bare({ name: 'shipname', placeholder: '名前' })
  const street = bare({ name: 'addr', placeholder: '住所' })
  const number = bare({ name: 'cardno', placeholder: 'カード番号' })
  const exp = bare({ name: 'expiry', placeholder: 'MM/YY' })
  const env = inLang(page([name, street, number, exp]), 'ja')
  const { cardOfferKinds } = classify(env)
  assert.deepEqual(cardOfferKinds(number), ['number', 'exp'])
  assert.equal(cardOfferKinds(name), null)

  const card = bare({ name: 'kartennummer', placeholder: 'Kartennummer' })
  const valid = bare({ name: 'ablaufdatum', placeholder: 'Gültig bis (MM/JJ)' })
  const strasse = bare({ name: 'strasse', placeholder: 'Straße' })
  const month = select(undefined, [['', 'Monat'], ['8', '8']])
  month.attrs = { name: 'geburtsmonat', 'aria-label': 'Geburtsmonat' }
  const year = select(undefined, [['', 'Jahr'], ['2028', '2028']])
  year.attrs = { name: 'geburtsjahr', 'aria-label': 'Geburtsjahr' }
  const de = inLang(page([card, valid, strasse, month, year]), 'de')
  assert.deepEqual(classify(de).cardOfferKinds(card), ['number', 'exp'])
})

test('beside a tagged card form nothing is guessed, so a security code box is never a number', () => {
  const number = field('cc-number')
  const code = bare({ name: 'f2', placeholder: 'Prüfnummer' })
  const env = inLang(page([number, code]), 'de')
  assert.equal(classify(env).cardOfferKinds(code), null)
  fill(page([number, code]))
  assert.equal(code.value, '')
})

test('one expiry date box is not split with the box after it', () => {
  const number = bare({ name: 'cardnumber', placeholder: 'Card number' })
  const exp = bare({ name: 'expiry', placeholder: 'Expiration date (MM/YY)' })
  const promo = bare({ name: 'express_code', placeholder: 'Promo' })
  assert.deepEqual(classify(page([number, exp, promo])).cardOfferKinds(number), ['number', 'exp'])
})
