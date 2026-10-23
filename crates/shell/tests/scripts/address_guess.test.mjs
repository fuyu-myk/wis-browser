// How `address_field` reads fields without an address `autocomplete` token,
// with Chromium's patterns.

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { includedFile, rawStringConst } from '../support/rustSource.mjs'

const CLASSIFY = [
  includedFile('address_field/patterns.js'),
  rawStringConst('address_field/guess.rs', 'GUESS_JS'),
  rawStringConst('address_field/scripts.rs', 'CLASSIFY_JS'),
].join('\n')
const FILL = rawStringConst('address_field/scripts.rs', 'FILL_JS')

const STYLE = {
  display: 'block',
  visibility: 'visible',
  opacity: '1',
  fontSize: '13px',
  color: 'rgb(0, 0, 0)',
  webkitTextFillColor: 'rgb(0, 0, 0)',
  overflowX: 'visible',
  overflowY: 'visible',
  position: 'static',
}

const text = (value) => ({ nodeType: 3, nodeValue: value })
const label = (...children) => ({
  nodeType: 1,
  tagName: 'LABEL',
  childNodes: children.map((child) => (typeof child === 'string' ? text(child) : child)),
})

/** A form control described by what a person or the page's markup says about it. */
function control({ tag = 'INPUT', type, words, attrs = {}, ...rest }) {
  const el = {
    nodeType: 1,
    tagName: tag,
    attrs: { ...(type ? { type } : {}), ...attrs },
    labels: words ? [label(words)] : [],
    disabled: false,
    readOnly: false,
    isConnected: true,
    value: '',
    maxLength: -1,
    form: null,
    parentElement: null,
    previousSibling: null,
    previousElementSibling: null,
    childNodes: [],
    style: STYLE,
    ...rest,
  }
  el.getAttribute = (name) => (name in el.attrs ? el.attrs[name] : null)
  el.getBoundingClientRect = () => el.box
  el.contains = (other) => other === el
  el.dispatchEvent = () => true
  if (tag === 'SELECT') el.options = el.options || []
  return el
}

/** A page holding `controls` in one form, stacked down the page. */
function page(controls, { lang = 'en', formless = false } = {}) {
  const form = formless ? null : {}
  controls.forEach((el, i) => {
    el.form = form
    el.box = { left: 10, top: 10 + 40 * i, right: 210, bottom: 40 + 40 * i, width: 200, height: 30 }
  })
  if (form) form.querySelectorAll = () => controls
  const at = (x, y) => (el) => x >= el.box.left && x < el.box.right && y >= el.box.top && y < el.box.bottom
  const doc = {
    activeElement: controls[0],
    documentElement: {
      lang,
      scrollWidth: 1200,
      scrollHeight: 5000,
      style: { overflowX: 'visible', overflowY: 'visible' },
    },
    querySelectorAll: () => controls,
    elementFromPoint: (x, y) => controls.find(at(x, y)) || null,
    getElementById: () => null,
  }
  const window = {
    scrollX: 0,
    scrollY: 0,
    innerWidth: 1280,
    innerHeight: 800,
    __wisAddressAnchor: controls[0],
    __wisAddressToken: 'offer-1',
  }
  const run = (body, extra = {}) =>
    new Function(
      'document',
      'window',
      'getComputedStyle',
      'location',
      'navigator',
      'Event',
      ...Object.keys(extra),
      `${CLASSIFY}\n${body}`,
    )(
      doc,
      window,
      (el) => el.style,
      { origin: 'https://shop.example' },
      { language: 'en' },
      class {},
      ...Object.values(extra),
    )
  const api = run('return { offerKinds, addressSection }')
  const kindOf = (el) => (api.addressSection(el) || []).find((field) => field.el === el)?.kind ?? null
  return { ...api, kindOf, run, doc, window }
}

const kinds = (env, el) => env.offerKinds(el)

test('a form labelled in words is read with Chromium patterns', () => {
  const fields = [
    control({ words: 'Full name' }),
    control({ words: 'Address line 1' }),
    control({ words: 'Address line 2' }),
    control({ words: 'Town or city' }),
    control({ words: 'Postcode' }),
    control({ tag: 'SELECT', words: 'Country' }),
    control({ type: 'tel', words: 'Phone number' }),
    control({ type: 'email', words: 'Email address' }),
  ]
  assert.deepEqual(kinds(page(fields), fields[0]), [
    'name',
    'line1',
    'line2',
    'city',
    'postal',
    'country',
    'tel',
    'email',
  ])
})

test('fields named only in their markup are read too', () => {
  const fields = [
    control({ attrs: { name: 'firstname' } }),
    control({ attrs: { id: 'lastname' } }),
    control({ attrs: { name: 'address1' } }),
    control({ attrs: { name: 'address2' } }),
    control({ attrs: { name: 'city' } }),
    control({ attrs: { name: 'state' } }),
    control({ attrs: { name: 'zip' } }),
  ]
  assert.deepEqual(kinds(page(fields), fields[2]), [
    'given',
    'family',
    'line1',
    'line2',
    'city',
    'region',
    'postal',
  ])
})

test('a second address line counts only right after the first', () => {
  const after = [
    control({ words: 'Street address' }),
    control({ words: 'Apartment, suite, etc.' }),
    control({ words: 'City' }),
  ]
  assert.equal(page(after).kindOf(after[1]), 'line2')

  const apart = [
    control({ words: 'Street address' }),
    control({ words: 'City' }),
    control({ words: 'Suite' }),
  ]
  assert.equal(page(apart).kindOf(apart[2]), null)
})

test('email, search, code and card fields are not taken for an address', () => {
  const card = control({ words: 'Card number' })
  const fields = [
    control({ type: 'email', words: 'Email address' }),
    control({ type: 'search', words: 'Search for a city' }),
    control({ words: 'Verification code' }),
    control({ words: 'Name on card' }),
    card,
    control({ words: 'Street address' }),
    control({ words: 'City' }),
    control({ words: 'Postcode' }),
  ]
  const env = page(fields)
  assert.deepEqual(
    fields.map((el) => env.kindOf(el)),
    ['email', null, null, null, null, 'line1', 'city', 'postal'],
  )

  const noCard = [control({ words: 'Name on card' }), control({ words: 'City' })]
  assert.equal(page(noCard, { lang: 'ja' }).kindOf(noCard[0]), null)
  const japanese = [control({ words: 'お名前' }), control({ words: '郵便番号' }), control({ words: '市区町村' })]
  assert.equal(page(japanese, { lang: 'ja' }).kindOf(japanese[0]), 'name', 'without a card, a name is a name')
  const paying = [...japanese.map((el) => control({ words: el.labels[0].childNodes[0].nodeValue })), control({ words: 'カード番号' })]
  assert.equal(page(paying, { lang: 'ja' }).kindOf(paying[0]), null, 'beside a card number, it may be the card\'s')
})

test('the same fields written out again start another section', () => {
  const shipping = [control({ words: 'Name' }), control({ words: 'Street' }), control({ words: 'City' })]
  const billing = [control({ words: 'Name' }), control({ words: 'Street' }), control({ words: 'City' })]
  const env = page([...shipping, ...billing])
  assert.deepEqual(
    env.addressSection(billing[1]).map((field) => field.el),
    billing,
  )
  assert.deepEqual(
    env.addressSection(shipping[2]).map((field) => field.el),
    shipping,
  )
})

test('a section read from words alone needs three kinds of field', () => {
  const two = [control({ words: 'City' }), control({ words: 'Postcode' })]
  assert.equal(kinds(page(two), two[0]), null)
  const three = [control({ words: 'City' }), control({ words: 'Postcode' }), control({ words: 'Street' })]
  assert.deepEqual(kinds(page(three), three[0]), ['city', 'postal', 'line1'])
})

test('a field tagged for something else is left alone, and off is no tag', () => {
  const fields = [
    control({ words: 'Postcode', attrs: { autocomplete: 'cc-number' } }),
    control({ words: 'Full name', attrs: { autocomplete: 'username' } }),
    control({ words: 'City', attrs: { autocomplete: 'off' } }),
    control({ words: 'Street', attrs: { autocomplete: 'nope' } }),
  ]
  const env = page(fields)
  assert.deepEqual(
    fields.map((el) => env.kindOf(el)),
    [null, null, 'city', 'line1'],
  )
})

test('the page language picks its patterns, with English for markup names', () => {
  const german = [
    control({ words: 'Postleitzahl' }),
    control({ words: 'Postcode' }),
    control({ attrs: { name: 'postcode' } }),
    control({ words: 'Ort' }),
  ]
  const env = page(german, { lang: 'de-AT' })
  assert.deepEqual(
    german.map((el) => env.kindOf(el)),
    ['postal', null, 'postal', 'city'],
  )

  const unknown = [control({ words: 'Postleitzahl' }), control({ words: 'Town' }), control({ words: 'Street' })]
  const any = page(unknown, { lang: 'xx' })
  assert.deepEqual(
    unknown.map((el) => any.kindOf(el)),
    ['postal', 'city', 'line1'],
  )
})

test('words come from labels without their options, from aria and from nearby text', () => {
  const option = { nodeType: 1, tagName: 'OPTION', childNodes: [text('Region of Murcia')] }
  const country = control({ tag: 'SELECT' })
  country.labels = [label('Country ', { nodeType: 1, tagName: 'SELECT', childNodes: [option] })]
  const town = control({ attrs: { 'aria-label': 'Town' } })
  const postal = control({})
  postal.previousSibling = { nodeType: 1, tagName: 'SPAN', childNodes: [text('Postcode')] }
  const street = control({ attrs: { placeholder: 'Street address' } })
  const env = page([country, town, postal, street])
  assert.deepEqual(
    [country, town, postal, street].map((el) => env.kindOf(el)),
    ['country', 'city', 'postal', 'line1'],
  )
})

test('a postal address is a street, not a postcode', () => {
  const fields = [
    control({ words: 'Full name' }),
    control({ words: 'Postal address' }),
    control({ words: 'Town' }),
    control({ words: 'Postcode' }),
  ]
  const env = page(fields)
  assert.deepEqual(kinds(env, fields[0]), ['name', 'line1', 'city', 'postal'])
})

test('nearby words are read as Chromium reads them, never from a heading', () => {
  const heading = { nodeType: 1, tagName: 'H3', childNodes: [text('Shipping address')] }
  const wrap = (el, before) => {
    el.parentElement = { nodeType: 1, tagName: 'DIV', previousSibling: before, childNodes: [el] }
    return el
  }
  const name = wrap(control({ attrs: { placeholder: 'Full name' } }), heading)
  const street = wrap(control({ attrs: { placeholder: 'Street' } }), null)
  const city = wrap(control({ attrs: { placeholder: 'City' } }), null)
  assert.deepEqual(
    [name, street, city].map((el) => page([name, street, city]).kindOf(el)),
    ['name', 'line1', 'city'],
  )

  const afterHeading = control({ previousSibling: { nodeType: 1, tagName: 'H3', childNodes: [text('Postcode')] } })
  const afterParagraph = control({ previousSibling: { nodeType: 1, tagName: 'P', childNodes: [text('Postcode')] } })
  const afterBold = control({
    previousSibling: text(' (required)'),
  })
  afterBold.previousSibling.previousSibling = { nodeType: 1, tagName: 'B', childNodes: [text('City')] }
  const env = page([afterHeading, afterParagraph, afterBold, control({ words: 'Street' })])
  assert.deepEqual(
    [afterHeading, afterParagraph, afterBold].map((el) => env.kindOf(el)),
    [null, 'postal', 'city'],
  )
})

test('a phone split over boxes, or a ZIP+4 box, leaves the section whole', () => {
  const split = [
    control({ words: 'Name' }),
    control({ words: 'Street' }),
    control({ words: 'City' }),
    control({ type: 'tel', words: 'Phone', attrs: { name: 'phone1' }, maxLength: 3 }),
    control({ type: 'tel', attrs: { name: 'phone2' }, maxLength: 3 }),
    control({ type: 'tel', attrs: { name: 'phone3' }, maxLength: 4 }),
    control({ type: 'email', words: 'Email' }),
  ]
  const env = page(split)
  assert.deepEqual(kinds(env, split[0]), ['name', 'line1', 'city', 'email'])

  const zip = [
    control({ words: 'Street' }),
    control({ words: 'City' }),
    control({ words: 'ZIP code' }),
    control({ attrs: { name: 'zip4' }, maxLength: 4 }),
    control({ type: 'tel', words: 'Phone' }),
  ]
  assert.deepEqual(kinds(page(zip), zip[0]), ['line1', 'city', 'postal', 'tel'])

  const labelled = [
    control({ words: 'Street' }),
    control({ type: 'tel', words: 'Phone', attrs: { name: 'phone_area' } }),
    control({ type: 'tel', words: 'Prefix', attrs: { name: 'phone_prefix' } }),
    control({ type: 'tel', words: 'Line', attrs: { name: 'phone_line' } }),
  ]
  const parts = page(labelled)
  assert.deepEqual(
    labelled.map((el) => parts.kindOf(el)),
    ['line1', null, null, null],
  )

  const whole = [control({ words: 'Phone number (including area code)' }), control({ words: 'City' })]
  assert.equal(page(whole).kindOf(whole[0]), 'tel')
})

test('fields outside a form group by the part of the page they sit in', () => {
  const header = { nodeType: 1, tagName: 'HEADER', querySelectorAll: () => [locator] }
  const locator = control({ words: 'Enter ZIP code', closest: () => header })
  const fields = [control({ words: 'Name' }), control({ words: 'Street' }), control({ words: 'City' })]
  const env = page([locator, ...fields], { formless: true })
  assert.deepEqual(kinds(env, fields[0]), ['name', 'line1', 'city'])
  assert.equal(env.addressSection(locator).length, 1)
})

test('a table form is read from the cell before the field', () => {
  const cell = (el, words) => {
    const label = { nodeType: 1, tagName: 'TD', childNodes: [text(words)] }
    el.parentElement = { nodeType: 1, tagName: 'TD', previousSibling: text('\n  '), childNodes: [el] }
    el.parentElement.previousSibling.previousSibling = label
    return el
  }
  const fields = [cell(control({}), 'Name'), cell(control({}), 'Street'), cell(control({}), 'Postcode')]
  const env = page(fields)
  assert.deepEqual(
    fields.map((el) => env.kindOf(el)),
    ['name', 'line1', 'postal'],
  )
})

test('only an email or a phone repeated in the next field stays in its section', () => {
  const fields = [
    control({ words: 'Address' }),
    control({ words: 'Postcode' }),
    control({ words: 'City' }),
    control({ words: 'Billing city' }),
    control({ words: 'Billing address' }),
    control({ words: 'Billing postcode' }),
  ]
  const env = page(fields)
  assert.deepEqual(
    env.addressSection(fields[3]).map((field) => field.el),
    fields.slice(3),
  )

  const emails = [control({ words: 'Email' }), control({ words: 'Confirm email' }), control({ words: 'Street' })]
  assert.equal(page(emails).addressSection(emails[1]).length, 3)
})

test('only a short box after a postcode is its ZIP+4, and two whole phones stay', () => {
  const zips = [control({ words: 'Shipping ZIP' }), control({ words: 'Billing ZIP' }), control({ words: 'Street' })]
  assert.equal(page(zips).kindOf(zips[1]), 'postal')

  const phones = [
    control({ type: 'tel', words: 'Home phone' }),
    control({ type: 'tel', words: 'Mobile phone' }),
    control({ words: 'Street' }),
  ]
  const env = page(phones)
  assert.deepEqual(
    phones.map((el) => env.kindOf(el)),
    ['tel', 'tel', 'line1'],
  )
})

test("a form's fields include those tied to it from elsewhere on the page", () => {
  const fields = [control({ words: 'Name' }), control({ words: 'Street' })]
  const outside = control({ words: 'City' })
  const env = page([...fields, outside])
  const form = fields[0].form
  form.tagName = 'FORM'
  form.elements = [...fields, outside]
  form.querySelectorAll = () => fields
  assert.equal(env.addressSection(fields[0]).length, 3)
})

test('a lone first or last name counts only as a full name', () => {
  const lone = [control({ words: 'First name' }), control({ words: 'Street' }), control({ words: 'City' })]
  assert.equal(page(lone).kindOf(lone[0]), null)
  const pair = [control({ words: 'First name' }), control({ words: 'Surname' }), control({ words: 'City' })]
  assert.deepEqual(
    pair.map((el) => page(pair).kindOf(el)),
    ['given', 'family', 'city'],
  )
})

test('a form read from words is filled like a tagged one', () => {
  const fields = [
    control({ words: 'First name' }),
    control({ words: 'Last name' }),
    control({ words: 'Address line 1' }),
    control({ words: 'Address line 2' }),
    control({ words: 'City' }),
    control({ words: 'Postcode' }),
  ]
  const env = page(fields)
  env.run('var ORIGIN = location.origin; var TOKEN = "offer-1";\n' + FILL, {
    V: {
      given: 'Ada',
      family: 'Lovelace',
      street: '12 St James Square\nFlat 3',
      city: 'London',
      postal: 'SW1Y 4JH',
    },
  })
  assert.deepEqual(
    fields.map((el) => el.value),
    ['Ada', 'Lovelace', '12 St James Square', 'Flat 3', 'London', 'SW1Y 4JH'],
  )
})
