// Which fields `vault/address_field/mod.rs` takes for an address, which it counts as
// shown, and how it fills them.

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { includedFile, rawStringConst } from '../support/rustSource.mjs'

const CLASSIFY = [
  includedFile('vault/address_field/patterns.js'),
  rawStringConst('vault/address_field/guess.rs', 'GUESS_JS'),
  rawStringConst('vault/address_field/scripts.rs', 'CLASSIFY_JS'),
].join('\n')
const FOCUS = rawStringConst('vault/address_field/scripts.rs', 'FOCUS_JS')
const FILL = rawStringConst('vault/address_field/scripts.rs', 'FILL_JS')

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

/** A form control with what the scripts read. */
function control(spec) {
  const el = {
    nodeType: 1,
    tagName: 'INPUT',
    attrs: {},
    disabled: false,
    readOnly: false,
    isConnected: true,
    value: '',
    maxLength: -1,
    form: null,
    parentElement: null,
    box: { left: 10, top: 10, right: 210, bottom: 40, width: 200, height: 30 },
    ...spec,
    style: { ...SHOWN_STYLE, ...(spec.style || {}) },
  }
  el.getAttribute = (name) => (name in el.attrs ? el.attrs[name] : null)
  el.getBoundingClientRect = () => el.box
  el.contains = (other) => other === el
  el.dispatchEvent = () => true
  if (el.tagName === 'SELECT') el.options = el.options || []
  return el
}

const field = (autocomplete, extra = {}) =>
  control({ ...extra, attrs: { autocomplete, ...(extra.attrs || {}) } })

/** A parent element with its own box, scroll state and style. */
function parent(spec) {
  const el = {
    nodeType: 1,
    parentElement: null,
    box: { left: 0, top: 0, right: 600, bottom: 600, width: 600, height: 600 },
    scrollLeft: 0,
    scrollTop: 0,
    clientLeft: 0,
    clientTop: 0,
    ...spec,
    style: { ...SHOWN_STYLE, ...(spec.style || {}) },
  }
  el.scrollWidth ??= el.box.width
  el.scrollHeight ??= el.box.height
  el.getBoundingClientRect = () => el.box
  el.contains = (other) => other === el
  return el
}

function page(
  controls,
  { anchor = controls[0], origin = 'https://shop.example', covers = [], token = 'offer-1' } = {},
) {
  const at = (x, y) => (el) => x >= el.box.left && x < el.box.right && y >= el.box.top && y < el.box.bottom
  const doc = {
    activeElement: anchor,
    documentElement: {
      lang: 'en',
      scrollWidth: 1200,
      scrollHeight: 5000,
      style: { overflowX: 'visible', overflowY: 'visible' },
    },
    querySelectorAll: () => controls,
    elementFromPoint: (x, y) => covers.find(at(x, y)) || controls.find(at(x, y)) || null,
  }
  for (const el of controls) {
    if (el.form) el.form.querySelectorAll = () => controls.filter((other) => other.form === el.form)
  }
  return {
    doc,
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
    `${CLASSIFY}\nreturn { addressField, shownField, offerKinds, worthOffering }`,
  )(env.doc, env.window, env.getComputedStyle)
}

const home = {
  given: 'Ada',
  family: 'Lovelace',
  organization: '',
  street: '12 St James Square\nFlat 3',
  city: 'London',
  region: '',
  postal: 'SW1Y 4JH',
  country: 'GB',
  tel: '+44 20 7946 0000',
  email: 'ada@example.org',
}

function fill(env, values = home, origin = 'https://shop.example', token = 'offer-1') {
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

test('an address field is known by its autocomplete token, with its section', () => {
  const { addressField } = classify(page([]))
  assert.deepEqual(addressField(field('shipping street-address')), { kind: 'street', section: 'shipping' })
  assert.deepEqual(addressField(field('section-a billing postal-code')), {
    kind: 'postal',
    section: 'section-a billing',
  })
  assert.deepEqual(addressField(field('given-name webauthn')), { kind: 'given', section: '' })
  assert.deepEqual(addressField(field('country', { tagName: 'SELECT' })), { kind: 'country', section: '' })
  for (const el of [
    field('off'),
    field('username'),
    field('email', { attrs: { type: 'password' } }),
    field('email', { disabled: true }),
    control({}),
  ]) {
    assert.equal(addressField(el), null)
  }
})

test('a lone email or phone field is not worth an address', () => {
  const { worthOffering } = classify(page([]))
  assert.equal(worthOffering(['email']), false)
  assert.equal(worthOffering(['email', 'tel']), false)
  assert.equal(worthOffering(['email', 'given', 'family']), true)
  assert.equal(worthOffering(['postal']), true)
})

test('a field is shown only when nothing hides, clips, shrinks, covers or fades it', () => {
  const style = (extra) => field('postal-code', { style: extra })
  const inside = (parentSpec) => field('postal-code', { parentElement: parent(parentSpec) })
  const hidden = {
    'opacity just above zero': style({ opacity: '0.01' }),
    'half faded': style({ opacity: '0.4' }),
    'text nearly transparent': style({ color: 'rgba(0, 0, 0, 0.02)' }),
    'text transparent in oklch': style({ color: 'oklch(0 0 0 / 0)' }),
    'blended away': style({ mixBlendMode: 'screen' }),
    'an svg filter': style({ filter: 'url(#hide)' }),
    'a painting-contained tiny parent': inside({
      box: { left: 10, top: 10, right: 11, bottom: 11, width: 1, height: 1 },
      style: { contain: 'paint' },
    }),
    'fixed below the viewport': field('postal-code', {
      style: { position: 'fixed' },
      box: { left: 10, top: 1600, right: 210, bottom: 1630, width: 200, height: 30 },
    }),
    'a faded parent': inside({ style: { opacity: '0.05' } }),
    'a parent with visibility hidden': inside({ style: { visibility: 'hidden' } }),
    'clip-path': style({ clipPath: 'inset(50%)' }),
    clip: style({ clip: 'rect(0px, 0px, 0px, 0px)' }),
    mask: style({ maskImage: 'linear-gradient(transparent, transparent)' }),
    'filter opacity': style({ filter: 'opacity(0)' }),
    'fixed below the viewport in a scrolling parent': field('postal-code', {
      style: { position: 'fixed' },
      parentElement: parent({ style: { overflowX: 'auto', overflowY: 'auto' }, scrollHeight: 5000 }),
      box: { left: 10, top: 1600, right: 210, bottom: 1630, width: 200, height: 30 },
    }),
    'absolute far left of a scrolling parent': field('postal-code', {
      style: { position: 'absolute' },
      parentElement: parent({ style: { overflowX: 'auto', overflowY: 'auto' } }),
      box: { left: -9999, top: 10, right: -9799, bottom: 40, width: 200, height: 30 },
    }),
    'above where a scrolling parent can scroll': field('postal-code', {
      parentElement: parent({
        box: { left: 0, top: 700, right: 600, bottom: 1300, width: 600, height: 600 },
        style: { overflowX: 'hidden', overflowY: 'auto' },
      }),
      box: { left: 10, top: 100, right: 210, bottom: 130, width: 200, height: 30 },
    }),
    'right to left, past a scrolling parent': field('postal-code', {
      parentElement: parent({
        box: { left: 0, top: 0, right: 300, bottom: 600, width: 300, height: 600 },
        style: { overflowX: 'auto', overflowY: 'hidden', direction: 'rtl' },
        scrollWidth: 2000,
      }),
      box: { left: 400, top: 10, right: 600, bottom: 40, width: 200, height: 30 },
    }),
    'absolute above where a scrolling parent can scroll, inside a transform': field('postal-code', {
      style: { position: 'absolute' },
      parentElement: parent({
        style: { transform: 'translateZ(0)' },
        parentElement: parent({
          box: { left: 0, top: 1000, right: 600, bottom: 1300, width: 600, height: 300 },
          style: { overflowX: 'hidden', overflowY: 'auto' },
          scrollHeight: 2000,
          parentElement: parent({ style: { position: 'relative' } }),
        }),
      }),
      box: { left: 10, top: 700, right: 210, bottom: 730, width: 200, height: 30 },
    }),
    'slotted into a faded shadow host': field('postal-code', {
      assignedSlot: parent({
        style: { display: 'contents' },
        parentNode: { nodeType: 11, host: parent({ style: { opacity: '0.05' } }) },
      }),
    }),
    'a tiny scrolling parent': inside({
      box: { left: 10, top: 10, right: 15, bottom: 15, width: 5, height: 5 },
      style: { overflowX: 'auto', overflowY: 'auto' },
    }),
    'a tiny clipping parent': inside({
      box: { left: 10, top: 10, right: 11, bottom: 11, width: 1, height: 1 },
      style: { overflow: 'hidden', overflowX: 'hidden', overflowY: 'hidden' },
    }),
    'zero font size': style({ fontSize: '0px' }),
    'transparent text': style({ color: 'rgba(0, 0, 0, 0)' }),
    'transparent fill colour': style({ webkitTextFillColor: 'transparent' }),
    'far below the page': field('postal-code', {
      box: { left: 10, top: 6000, right: 210, bottom: 6030, width: 200, height: 30 },
    }),
    'far right of the page': field('postal-code', {
      box: { left: 3000, top: 10, right: 3200, bottom: 40, width: 200, height: 30 },
    }),
    'aria-hidden': field('postal-code', { attrs: { 'aria-hidden': 'true' } }),
    'display none': style({ display: 'none' }),
    'tiny box': field('postal-code', { box: { left: 0, top: 0, right: 5, bottom: 5, width: 5, height: 5 } }),
  }
  for (const [how, el] of Object.entries(hidden)) {
    const { shownField } = classify(page([el], { anchor: null }))
    assert.equal(shownField(el, null), false, how)
  }

  const plain = field('postal-code')
  assert.equal(classify(page([plain], { anchor: null })).shownField(plain, null), true)

  const below = field('postal-code', {
    box: { left: 10, top: 1600, right: 210, bottom: 1630, width: 200, height: 30 },
  })
  const scrolled = page([below], { anchor: null })
  assert.equal(classify(scrolled).shownField(below, null), true, 'below the fold of a page that scrolls')
  scrolled.doc.body = parent({
    box: { left: 0, top: 0, right: 1280, bottom: 800, width: 1280, height: 800 },
    style: { overflowX: 'hidden', overflowY: 'auto' },
  })
  below.parentElement = scrolled.doc.body
  assert.equal(classify(scrolled).shownField(below, null), true, 'a body whose overflow is the page\'s')
  scrolled.doc.body.style.overflowY = 'hidden'
  assert.equal(classify(scrolled).shownField(below, null), false, 'below the fold of a page that does not scroll')

  const scroller = parent({
    box: { left: 0, top: 0, right: 400, bottom: 300, width: 400, height: 300 },
    style: { overflowX: 'hidden', overflowY: 'auto' },
  })
  scroller.parentElement = parent({
    box: { left: 0, top: 0, right: 400, bottom: 300, width: 400, height: 300 },
    style: { overflowX: 'hidden', overflowY: 'hidden' },
  })
  scroller.scrollHeight = 1200
  const further = field('postal-code', {
    parentElement: scroller,
    box: { left: 10, top: 900, right: 210, bottom: 930, width: 200, height: 30 },
  })
  assert.equal(
    classify(page([further], { anchor: null })).shownField(further, null),
    true,
    'further down a box that scrolls, inside one that clips',
  )
  const reverseRow = field('postal-code', {
    parentElement: parent({
      box: { left: 0, top: 0, right: 400, bottom: 300, width: 400, height: 300 },
      style: { display: 'flex', flexDirection: 'row-reverse', overflowX: 'hidden', overflowY: 'auto' },
      scrollHeight: 1200,
    }),
    box: { left: 10, top: 900, right: 210, bottom: 930, width: 200, height: 30 },
  })
  assert.equal(
    classify(page([reverseRow], { anchor: null })).shownField(reverseRow, null),
    true,
    'further down a reversed row that scrolls down',
  )
  const fixedInTransform = field('postal-code', {
    style: { position: 'fixed' },
    parentElement: parent({
      box: { left: 0, top: 0, right: 400, bottom: 300, width: 400, height: 300 },
      style: { transform: 'translateZ(0)', overflowX: 'hidden', overflowY: 'auto' },
      scrollHeight: 1200,
    }),
    box: { left: 10, top: 900, right: 210, bottom: 930, width: 200, height: 30 },
  })
  assert.equal(
    classify(page([fixedInTransform], { anchor: null })).shownField(fixedInTransform, null),
    true,
    'fixed inside a transformed box that scrolls',
  )
  const hidingCover = { box: { left: 0, top: 0, right: 400, bottom: 300 } }
  assert.equal(
    classify(page([further], { anchor: null, covers: [hidingCover] })).shownField(further, null),
    false,
    'further down a box that scrolls, under a cover',
  )

  const covered = field('postal-code')
  const cover = { box: { left: 0, top: 0, right: 400, bottom: 100 } }
  const env = page([covered], { anchor: null, covers: [cover] })
  assert.equal(classify(env).shownField(covered, null), false, 'covered by another element')
  assert.equal(classify(env).shownField(covered, covered), true, 'the field the user is in')
})

test('only shown fields make a section worth an address, and the focused one must be shown', () => {
  const form = {}
  const newsletter = field('email', { form })
  const hiddenStreet = field('street-address', { form, style: { opacity: '0' } })
  assert.equal(classify(page([newsletter, hiddenStreet])).offerKinds(newsletter), null)

  const hiddenFocus = field('postal-code', { form, style: { display: 'none' } })
  assert.equal(classify(page([hiddenFocus])).offerKinds(hiddenFocus), null)

  const postal = field('postal-code', { form })
  const city = field('address-level2', {
    form,
    box: { left: 10, top: 60, right: 210, bottom: 90, width: 200, height: 30 },
  })
  const styledAway = field('country', { form, tagName: 'SELECT', style: { display: 'none' } })
  const fadedPhone = field('tel', { form, style: { opacity: '0.01' } })
  assert.deepEqual(classify(page([postal, city, styledAway, fadedPhone])).offerKinds(postal), [
    'postal',
    'city',
    'country',
  ])
})

test('beside a country, hidden address fields are offered too, as pages reveal them for it', () => {
  const form = {}
  const name = field('name', { form })
  const street = field('street-address', {
    form,
    box: { left: 10, top: 60, right: 210, bottom: 90, width: 200, height: 30 },
  })
  const country = field('country', {
    form,
    tagName: 'SELECT',
    box: { left: 10, top: 100, right: 210, bottom: 130, width: 200, height: 30 },
  })
  const postal = field('postal-code', { form, style: { display: 'none' } })
  const phone = field('tel', { form, style: { display: 'none' } })
  assert.deepEqual(classify(page([name, street, country, postal, phone])).offerKinds(name), [
    'name',
    'street',
    'country',
    'postal',
  ])

  const honeypot = field('street-address', { form: {}, style: { display: 'none' } })
  const beforeCountry = [
    honeypot,
    field('name', { form: honeypot.form, box: { left: 10, top: 60, right: 210, bottom: 90, width: 200, height: 30 } }),
    field('country', {
      form: honeypot.form,
      tagName: 'SELECT',
      box: { left: 10, top: 100, right: 210, bottom: 130, width: 200, height: 30 },
    }),
    field('postal-code', { form: honeypot.form, box: { left: 10, top: 140, right: 210, bottom: 170, width: 200, height: 30 } }),
  ]
  assert.deepEqual(classify(page(beforeCountry, { anchor: beforeCountry[1] })).offerKinds(beforeCountry[1]), [
    'name',
    'country',
    'postal',
  ], 'not a hidden field before the country')

  const chosen = field('country', {
    form: {},
    tagName: 'SELECT',
    value: 'GB',
    selectedIndex: 1,
    options: [{ value: '', text: 'Choose' }, { value: 'GB', text: 'United Kingdom' }],
  })
  const afterChoice = [
    field('name', { form: chosen.form, box: { left: 10, top: 60, right: 210, bottom: 90, width: 200, height: 30 } }),
    field('street-address', { form: chosen.form, box: { left: 10, top: 100, right: 210, bottom: 130, width: 200, height: 30 } }),
    chosen,
    field('postal-code', { form: chosen.form, style: { display: 'none' } }),
  ]
  assert.deepEqual(classify(page(afterChoice)).offerKinds(afterChoice[0]), [
    'name',
    'street',
    'country',
  ], 'not once a country is chosen')

  const noCountry = [field('name', { form: {} }), field('postal-code', { style: { display: 'none' } })]
  noCountry[1].form = noCountry[0].form
  noCountry.push(field('email', { form: noCountry[0].form, box: { left: 10, top: 60, right: 210, bottom: 90, width: 200, height: 30 } }))
  assert.deepEqual(classify(page(noCountry)).offerKinds(noCountry[0]), null)
})

test('the anchored section is filled, street lines shaped to each field', () => {
  const form = {}
  const controls = [
    field('name', { form }),
    field('street-address', { form, tagName: 'TEXTAREA', box: { left: 10, top: 50, right: 210, bottom: 90, width: 200, height: 40 } }),
    field('address-line1', { form, box: { left: 10, top: 100, right: 210, bottom: 130, width: 200, height: 30 } }),
    field('address-line2', { form, box: { left: 10, top: 140, right: 210, bottom: 170, width: 200, height: 30 } }),
    field('address-level2', { form, box: { left: 10, top: 180, right: 210, bottom: 210, width: 200, height: 30 } }),
    field('postal-code', { form, box: { left: 10, top: 220, right: 210, bottom: 250, width: 200, height: 30 } }),
    field('email', { form, box: { left: 10, top: 260, right: 210, bottom: 290, width: 200, height: 30 } }),
    field('billing postal-code', { form, box: { left: 10, top: 300, right: 210, bottom: 330, width: 200, height: 30 } }),
  ]
  fill(page(controls))
  assert.deepEqual(
    controls.map((el) => el.value),
    [
      'Ada Lovelace',
      '12 St James Square\nFlat 3',
      '12 St James Square',
      'Flat 3',
      'London',
      'SW1Y 4JH',
      'ada@example.org',
      '',
    ],
  )
})

test('only the values sent are filled, and filled fields are left alone', () => {
  const form = {}
  const postal = field('postal-code', { form, value: 'SW' })
  const city = field('address-level2', { form, value: 'Paris', box: { left: 10, top: 60, right: 210, bottom: 90, width: 200, height: 30 } })
  const phone = field('tel', { form, box: { left: 10, top: 100, right: 210, bottom: 130, width: 200, height: 30 } })
  fill(page([postal, city, phone]), { ...home, tel: '' })
  assert.equal(postal.value, 'SW1Y 4JH')
  assert.equal(city.value, 'Paris')
  assert.equal(phone.value, '')
})

test('lists get their own value, by code or name, and a choice already made stays', () => {
  const form = {}
  const option = (value, text) => ({ value, text })
  const postal = field('postal-code', { form })
  const countries = field('country', {
    form,
    tagName: 'SELECT',
    style: { display: 'none' },
    options: [option('', 'Choose'), option('FR', 'France'), option('uk', 'United Kingdom')],
  })
  const cities = field('address-level2', {
    form,
    tagName: 'SELECT',
    box: { left: 10, top: 60, right: 210, bottom: 90, width: 200, height: 30 },
    options: [option('', 'Choose'), option('ldn', 'London'), option('glx', 'Greater London')],
  })
  const regions = field('address-level1', {
    form,
    tagName: 'SELECT',
    value: 'NY',
    selectedIndex: 2,
    box: { left: 10, top: 100, right: 210, bottom: 130, width: 200, height: 30 },
    options: [option('', 'Choose'), option('CA', 'California'), option('NY', 'New York')],
  })
  fill(page([postal, countries, cities, regions]), { ...home, region: 'california' })
  assert.equal(countries.value, 'uk')
  assert.equal(cities.value, 'ldn')
  assert.equal(regions.value, 'NY')

  const defaulted = field('country', {
    form,
    tagName: 'SELECT',
    value: 'US',
    selectedIndex: 0,
    box: { left: 10, top: 60, right: 210, bottom: 90, width: 200, height: 30 },
    options: [{ value: 'US', text: 'United States', defaultSelected: true }, option('GB', 'United Kingdom')],
  })
  fill(page([field('postal-code', { form }), defaulted]))
  assert.equal(defaulted.value, 'GB', 'the page default is not a choice')
})

test('a country text field gets the code when it only takes two letters, else the name', () => {
  const form = {}
  const short = field('country', { form, maxLength: 2, box: { left: 10, top: 60, right: 210, bottom: 90, width: 200, height: 30 } })
  const long = field('country-name', { form, box: { left: 10, top: 100, right: 210, bottom: 130, width: 200, height: 30 } })
  const tooShort = field('address-level2', { form, maxLength: 3, box: { left: 10, top: 140, right: 210, bottom: 170, width: 200, height: 30 } })
  fill(page([field('postal-code', { form }), short, long, tooShort]))
  assert.equal(short.value, 'GB')
  assert.equal(long.value, 'United Kingdom')
  assert.equal(tooShort.value, '')
})

test('nothing is filled on another origin, or when the field the card was for has gone', () => {
  const form = {}
  const make = () => [field('postal-code', { form }), field('address-level2', { form, box: { left: 10, top: 60, right: 210, bottom: 90, width: 200, height: 30 } })]

  const elsewhere = make()
  fill(page(elsewhere, { origin: 'https://evil.example' }))
  assert.deepEqual(elsewhere.map((el) => el.value), ['', ''])

  const insecure = make()
  fill(page(insecure, { origin: 'http://shop.example' }))
  assert.deepEqual(insecure.map((el) => el.value), ['', ''])

  const moved = make()
  const env = page(moved)
  env.doc.activeElement = moved[1]
  fill(env)
  assert.deepEqual(moved.map((el) => el.value), ['', ''])

  const detached = make()
  detached[0].isConnected = false
  fill(page(detached))
  assert.deepEqual(detached.map((el) => el.value), ['', ''])

  const stale = make()
  fill(page(stale, { token: 'offer-2' }))
  assert.deepEqual(stale.map((el) => el.value), ['', ''], 'the card showed an older offer')
})

/** A page running the focus reporter, with a postal-code field and a search box. */
function reporter() {
  const sent = []
  const listeners = {}
  const timers = new Map()
  let nextTimer = 1
  const form = {}
  const postal = field('postal-code', { form })
  const city = field('address-level2', { form, box: { left: 10, top: 60, right: 210, bottom: 90, width: 200, height: 30 } })
  const search = control({ attrs: { type: 'search' }, form, box: { left: 10, top: 100, right: 210, bottom: 130, width: 200, height: 30 } })
  const controls = [postal, city, search]
  form.querySelectorAll = () => controls
  let counter = 0
  let clock = 0
  const window = {
    innerWidth: 1280,
    innerHeight: 800,
    scrollX: 0,
    scrollY: 0,
    crypto: {
      getRandomValues: (bytes) => {
        counter += 1
        return bytes.fill(counter)
      },
    },
    webkit: {
      messageHandlers: { wisAddress: { postMessage: (body) => sent.push(JSON.parse(body)) } },
    },
    addEventListener: (type, fn) => (listeners[type] ||= []).push(fn),
  }
  window.top = window.self = window
  const document = {
    activeElement: null,
    documentElement: {
      lang: 'en',
      scrollWidth: 1200,
      scrollHeight: 5000,
      style: { overflowX: 'visible', overflowY: 'visible' },
    },
    addEventListener: (type, fn) => (listeners[`doc:${type}`] ||= []).push(fn),
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
  )(
    window,
    document,
    (el) => el.style,
    (fn) => {
      const id = nextTimer++
      timers.set(id, fn)
      return id
    },
    (id) => timers.delete(id),
    { now: () => clock },
  )
  const later = (ms) => {
    clock += ms
    const due = [...timers.values()]
    timers.clear()
    due.forEach((fn) => fn())
  }
  const focus = (el, isTrusted = true) => {
    document.activeElement = el
    ;(listeners['doc:focusin'] || []).forEach((fn) => fn({ target: el, isTrusted }))
  }
  const fire = (key, event = {}) => (listeners[key] || []).forEach((fn) => fn(event))
  return { sent, postal, city, search, focus, fire, later, window }
}

test('a real focus on an address field is reported with its box and what it would fill', () => {
  const page = reporter()
  page.focus(page.search)
  assert.deepEqual(page.sent, [])

  page.focus(page.postal)
  const [report] = page.sent
  assert.match(report.token, /^[0-9a-f]{32}$/)
  assert.deepEqual(report, {
    kind: 'focus',
    x: 10,
    y: 10,
    width: 200,
    height: 30,
    viewWidth: 1280,
    kinds: ['postal', 'city'],
    token: report.token,
  })
  assert.equal(page.window.__wisAddressAnchor, page.postal)
  assert.equal(page.window.__wisAddressToken, report.token)

  page.fire('doc:pointerdown', { target: page.postal, isTrusted: true })
  assert.equal(page.sent.length, 1, 'reports are spaced apart')
  page.later(300)
  assert.equal(page.sent.length, 2, 'clicking the field again reports again')
  assert.equal(page.sent[1].token, report.token, 'the same offer keeps its token')

  page.city.style.display = 'none'
  page.later(300)
  page.fire('doc:pointerdown', { target: page.postal, isTrusted: true })
  assert.deepEqual(page.sent[2].kinds, ['postal'])
  assert.notEqual(page.sent[2].token, report.token, 'a different offer gets a new token')
})

test('a click elsewhere does not drop a report waiting its turn', () => {
  const page = reporter()
  page.focus(page.postal)
  page.focus(page.search)
  page.later(100)
  page.focus(page.postal)
  page.fire('doc:pointerdown', { target: { getAttribute: () => null, tagName: 'DIV' }, isTrusted: true })
  page.later(300)
  assert.equal(page.sent.filter((body) => body.kind === 'focus').length, 2)
})

test('a focus event the page made up is not reported', () => {
  const page = reporter()
  page.focus(page.postal, false)
  assert.deepEqual(page.sent, [])
  assert.equal(page.window.__wisAddressAnchor, undefined)
})

test('moving on, Escape or typing in the field lets the card go', () => {
  for (const leaveBy of [
    (page) => page.focus(page.search),
    (page) => page.fire('doc:keydown', { target: page.postal, key: 'Escape' }),
    (page) => page.fire('doc:input', { target: page.postal, isTrusted: true }),
  ]) {
    const page = reporter()
    page.focus(page.postal)
    leaveBy(page)
    assert.deepEqual(page.sent.at(-1), { kind: 'leave' })
    assert.equal(page.window.__wisAddressAnchor, null)
  }

  const page = reporter()
  page.focus(page.postal)
  page.fire('doc:input', { target: page.postal, isTrusted: false })
  assert.equal(page.sent.at(-1).kind, 'focus')
})
