// How `address_field` reports an address a person typed when its form is
// submitted, for "Save address?".

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'

import { includedFile, rawStringConst } from '../support/rustSource.mjs'

const SCRIPT = [
  includedFile('vault/address_field/patterns.js'),
  rawStringConst('vault/address_field/guess.rs', 'GUESS_JS'),
  rawStringConst('vault/address_field/scripts.rs', 'CLASSIFY_JS'),
  rawStringConst('vault/address_field/card_scripts.rs', 'CARD_JS'),
  rawStringConst('vault/address_field/scripts.rs', 'FOCUS_JS'),
  rawStringConst('vault/address_field/submit.rs', 'SUBMIT_JS'),
].join('\n')

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

/** A page with one form, running the address page script. */
function page() {
  const sent = []
  const listeners = {}
  const controls = []
  const form = { nodeType: 1, tagName: 'FORM', querySelectorAll: () => controls }
  const field = (autocomplete, value, extra = {}) => {
    const el = {
      nodeType: 1,
      tagName: 'INPUT',
      attrs: { autocomplete },
      labels: [],
      childNodes: [],
      disabled: false,
      readOnly: false,
      isConnected: true,
      maxLength: -1,
      form,
      parentElement: null,
      previousSibling: null,
      style: STYLE,
      value,
      ...extra,
    }
    el.getAttribute = (name) => (name in el.attrs ? el.attrs[name] : null)
    el.getBoundingClientRect = () => el.box
    el.contains = (other) => other === el
    controls.push(el)
    el.box = { left: 10, top: 40 * controls.length, right: 210, bottom: 40 * controls.length + 30, width: 200, height: 30 }
    return el
  }
  const list = (autocomplete, options, value) => {
    const el = field(autocomplete, value, { tagName: 'SELECT' })
    el.options = options.map(([v, text]) => ({ value: v, text }))
    el.selectedIndex = el.options.findIndex((option) => option.value === value)
    return el
  }
  const at = (x, y) => (el) => x >= el.box.left && x < el.box.right && y >= el.box.top && y < el.box.bottom
  const window = {
    innerWidth: 1280,
    innerHeight: 800,
    scrollX: 0,
    scrollY: 0,
    crypto: { getRandomValues: (bytes) => bytes.fill(1) },
    webkit: { messageHandlers: { wisAddress: { postMessage: (body) => sent.push(JSON.parse(body)) } } },
    addEventListener: (type, fn) => (listeners[type] ||= []).push(fn),
  }
  window.top = window.self = window
  const document = {
    activeElement: null,
    documentElement: { lang: 'en', scrollWidth: 1200, scrollHeight: 5000, style: STYLE },
    addEventListener: (type, fn) => (listeners[`doc:${type}`] ||= []).push(fn),
    querySelectorAll: () => controls,
    elementFromPoint: (x, y) => controls.find(at(x, y)) || null,
  }
  const start = () =>
    new Function('window', 'document', 'getComputedStyle', 'setTimeout', 'clearTimeout', 'performance', 'navigator', SCRIPT)(
      window,
      document,
      (el) => el.style,
      () => 0,
      () => undefined,
      { now: () => 0 },
      { language: 'en' },
    )
  const fire = (key, event) => (listeners[key] || []).forEach((fn) => fn(event))
  const saves = () => sent.filter((body) => body.kind === 'save')
  /** The person types into each of `els`. */
  const type = (...els) => els.forEach((el) => fire('doc:input', { target: el, isTrusted: true }))
  /** A real submit, right after a click. */
  const submit = () => {
    fire('doc:pointerdown', { target: form, isTrusted: true })
    fire('doc:submit', { target: form, isTrusted: true })
  }
  return { form, field, list, start, fire, saves, type, submit, window }
}

test('a real submit reports the address typed into the shown fields', () => {
  const env = page()
  env.field('given-name', 'Ada')
  env.field('family-name', 'Lovelace')
  const line1 = env.field('address-line1', '12 St James Square')
  const line2 = env.field('address-line2', 'Flat 3')
  env.field('address-line3', '')
  const city = env.field('address-level2', 'London')
  const postal = env.field('postal-code', ' SW1Y 4JH ')
  env.list('country', [['', 'Choose'], ['GB', 'United Kingdom']], 'GB')
  env.field('email', 'ada@example.org', { style: { ...STYLE, display: 'none' } })
  env.start()
  env.type(line1, line2, city, postal)
  env.submit()
  assert.deepEqual(env.saves(), [
    {
      kind: 'save',
      values: {
        given: 'Ada',
        family: 'Lovelace',
        street: '12 St James Square\nFlat 3',
        city: 'London',
        postal: 'SW1Y 4JH',
        country: 'GB',
      },
    },
  ])
})

test('a made-up submit, or a form without a street and a city or postcode, reports nothing', () => {
  const madeUp = page()
  const street = madeUp.field('street-address', '1 Road')
  const town = madeUp.field('address-level2', 'Leeds')
  madeUp.start()
  madeUp.type(street, town)
  madeUp.fire('doc:submit', { target: madeUp.form, isTrusted: false })
  assert.deepEqual(madeUp.saves(), [])

  const signIn = page()
  const email = signIn.field('email', 'ada@example.org')
  const name = signIn.field('name', 'Ada Lovelace')
  const city = signIn.field('address-level2', 'Leeds')
  signIn.start()
  signIn.type(email, name, city)
  signIn.submit()
  assert.deepEqual(signIn.saves(), [])
})

test('only what the person typed counts, and only a submit they set off', () => {
  const planted = page()
  planted.field('street-address', '1 Road')
  planted.field('address-level2', 'Leeds')
  planted.start()
  planted.submit()
  assert.deepEqual(planted.saves(), [], 'values the page put there')

  const half = page()
  const street = half.field('street-address', '1 Road')
  half.field('address-level2', 'Leeds')
  half.start()
  half.type(street)
  half.submit()
  assert.deepEqual(half.saves(), [], 'a city or postcode the page put there')

  const scripted = page()
  const road = scripted.field('street-address', '1 Road')
  const town = scripted.field('address-level2', 'Leeds')
  scripted.start()
  scripted.type(road, town)
  scripted.fire('doc:submit', { target: scripted.form, isTrusted: true })
  assert.deepEqual(scripted.saves(), [], 'requestSubmit with no click or key before it')
})

test('a click on an order button or Enter in an address field reports, once per address', () => {
  const env = page()
  const street = env.field('street-address', '1 Road')
  const postal = env.field('postal-code', 'LS1 4AP')
  env.start()
  env.type(street, postal)
  const label = { nodeType: 3, nodeValue: 'Place order' }
  const button = {
    nodeType: 1,
    tagName: 'BUTTON',
    form: env.form,
    attrs: {},
    childNodes: [label],
    textContent: 'Place order',
    getAttribute: () => null,
  }
  env.fire('doc:click', { target: button, isTrusted: true })
  env.fire('doc:keydown', { target: postal, key: 'Enter', isTrusted: true })
  assert.equal(env.saves().length, 1)

  postal.value = 'LS2 7HY'
  env.fire('doc:keydown', { target: postal, key: 'Enter', isTrusted: true })
  assert.equal(env.saves().length, 2)
  assert.equal(env.saves()[1].values.postal, 'LS2 7HY')

  postal.value = 'LS3 1AA'
  env.fire('doc:keydown', { target: postal, key: 'Enter', isTrusted: true, defaultPrevented: true })
  assert.equal(env.saves().length, 2, 'not an Enter the page took, as for a suggestion')

  const membership = { ...button, tagName: 'A', form: undefined, childNodes: [{ nodeType: 3, nodeValue: 'Membership' }] }
  membership.parentElement = null
  env.fire('doc:click', { target: membership, isTrusted: true })
  assert.equal(env.saves().length, 2, 'not a link whose words only contain a submit word')
})

test('a form still holding what Wis filled reports nothing, as the page saw that fill', () => {
  const env = page()
  const street = env.field('street-address', '1 Road')
  const city = env.field('address-level2', 'Leeds')
  env.start()
  env.type(street, city)
  env.window.__wisAddressWritten = new WeakMap([
    [street, { fill: 1, value: '1 Road' }],
    [city, { fill: 1, value: 'Leeds' }],
  ])
  env.submit()
  assert.deepEqual(env.saves(), [])

  city.value = 'Leeds 6'
  env.submit()
  assert.equal(env.saves().length, 1, 'once something differs')
})

test('Enter in a street box with lines is a new line, not a submit', () => {
  const env = page()
  const street = env.field('street-address', '1 Road', { tagName: 'TEXTAREA' })
  const town = env.field('address-level2', 'Leeds')
  env.start()
  env.type(street, town)
  env.fire('doc:keydown', { target: street, key: 'Enter', isTrusted: true })
  assert.deepEqual(env.saves(), [])
})

test('a country typed as a name is sent as its code, a region list as its name and code', () => {
  const env = page()
  const street = env.field('street-address', '1 Main Street')
  const city = env.field('address-level2', 'Albany')
  env.list('address-level1', [['', 'State'], ['CA', 'California'], ['NY', 'New York']], 'NY')
  env.field('country-name', 'United States')
  env.start()
  env.type(street, city)
  env.submit()
  const [save] = env.saves()
  assert.equal(save.values.country, 'US')
  assert.equal(save.values.region, 'New York')
  assert.equal(save.values.regionCode, 'NY')
})

test('the country codes match the ones Settings offers', () => {
  const settings = readFileSync(new URL('../../src/features/vault/lib/addresses.ts', import.meta.url), 'utf8')
  const codes = (source) => source.match(/\b[A-Z]{2}\b/g)
  const listed = codes(settings.slice(settings.indexOf('COUNTRY_CODES'), settings.indexOf(".split(' ')")))
  const script = rawStringConst('vault/address_field/submit.rs', 'SUBMIT_JS')
  const shipped = codes(script.slice(script.indexOf('COUNTRY_CODES'), script.indexOf(".split(' ')")))
  assert.deepEqual(shipped, listed)
})
