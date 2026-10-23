// How `address_field` fills a form that changes as it is filled: the country
// first, then one more pass once the form settles.

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { includedFile, rawStringConst } from '../support/rustSource.mjs'

const SCRIPT = [
  includedFile('vault/address_field/patterns.js'),
  rawStringConst('vault/address_field/guess.rs', 'GUESS_JS'),
  rawStringConst('vault/address_field/scripts.rs', 'CLASSIFY_JS'),
  rawStringConst('vault/address_field/scripts.rs', 'FILL_JS'),
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

const VALUES = { given: 'Ada', family: 'Lovelace', city: 'New York', region: 'NY', postal: '10001', country: 'US' }

/** A page with a form whose fields can change, and the clock and observers it runs on. */
function page() {
  const log = []
  const controls = []
  const form = { querySelectorAll: () => controls.filter((el) => el.isConnected) }
  const field = (autocomplete, extra = {}) => {
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
      value: '',
      ...extra,
    }
    el.getAttribute = (name) => (name in el.attrs ? el.attrs[name] : null)
    el.getBoundingClientRect = () => el.box
    el.contains = (other) => other === el
    el.dispatchEvent = (event) => {
      log.push(`${autocomplete}:${event.type}`)
      if (event.type === 'change') el.onchange?.()
      return true
    }
    controls.push(el)
    el.box = { left: 10, top: 10 + 40 * controls.length, right: 210, bottom: 40 + 40 * controls.length, width: 200, height: 30 }
    return el
  }
  const list = (autocomplete, options) => {
    const el = field(autocomplete, { tagName: 'SELECT' })
    el.options = options.map(([value, text]) => ({ value, text }))
    el.selectedIndex = 0
    Object.defineProperty(el, 'value', {
      get: () => el.options[el.selectedIndex]?.value ?? '',
      set: (value) => {
        el.selectedIndex = el.options.findIndex((option) => option.value === value)
      },
    })
    return el
  }

  let clock = 0
  const timers = []
  const observers = []
  class Observer {
    constructor(callback) {
      this.callback = callback
      observers.push(this)
    }
    observe() {
      this.live = true
    }
    disconnect() {
      this.live = false
    }
  }
  const later = (ms) => {
    const until = clock + ms
    for (;;) {
      const due = timers.filter((timer) => !timer.done && timer.at <= until).sort((a, b) => a.at - b.at)[0]
      if (!due) break
      clock = due.at
      due.done = true
      due.fn()
    }
    clock = until
  }
  const at = (x, y) => (el) =>
    el.isConnected && x >= el.box.left && x < el.box.right && y >= el.box.top && y < el.box.bottom
  const doc = {
    documentElement: { lang: 'en', scrollWidth: 1200, scrollHeight: 5000, style: STYLE },
    querySelectorAll: () => controls,
    elementFromPoint: (x, y) => controls.find(at(x, y)) || null,
  }
  const window = { scrollX: 0, scrollY: 0, innerWidth: 1280, innerHeight: 800, __wisAddressToken: 'offer-1' }
  const location = { origin: 'https://shop.example' }

  const fill = (values = VALUES) => {
    window.__wisAddressAnchor = controls[0]
    doc.activeElement = controls[0]
    window.__wisAddressToken = 'offer-1'
    new Function(
      'document',
      'location',
      'window',
      'getComputedStyle',
      'navigator',
      'Event',
      'MutationObserver',
      'setTimeout',
      'clearTimeout',
      'ORIGIN',
      'TOKEN',
      'V',
      SCRIPT,
    )(
      doc,
      location,
      window,
      (el) => el.style,
      { language: 'en' },
      class {
        constructor(type) {
          this.type = type
        }
      },
      Observer,
      (fn, ms) => timers.push({ at: clock + ms, fn }),
      (id) => id && timers[id - 1] && (timers[id - 1].done = true),
      'https://shop.example',
      'offer-1',
      values,
    )
    // The card closing releases the field, as it does after a pick.
    window.__wisAddressToken = null
  }
  const mutate = () => observers.filter((observer) => observer.live).forEach((observer) => observer.callback([]))

  const watching = () => observers.some((observer) => observer.live)

  return { log, field, list, fill, later, mutate, watching, window, location }
}

test('the country is filled first, and the rest after the page redraws for it', () => {
  const env = page()
  const postal = env.field('postal-code')
  const region = env.list('address-level1', [['', 'Choose']])
  const country = env.list('country', [['', 'Choose'], ['GB', 'United Kingdom'], ['US', 'United States']])
  country.onchange = () => {
    region.options = [['', 'Choose'], ['CA', 'California'], ['NY', 'New York']].map(([value, text]) => ({ value, text }))
  }
  env.fill()
  assert.equal(env.log[0], 'country:input', 'the country goes first')
  assert.equal(country.value, 'US')
  assert.equal(region.value, 'NY', 'the list the country brought is filled')
  assert.equal(postal.value, '10001')
})

test('a postcode field the country reveals is filled', () => {
  const env = page()
  env.field('name')
  const country = env.list('country', [['', 'Choose'], ['SG', 'Singapore'], ['US', 'United States']])
  const postal = env.field('postal-code', { style: { ...STYLE, display: 'none' } })
  country.onchange = () => {
    postal.style = STYLE
  }
  env.fill({ name: 'Ada Lovelace', country: 'SG', postal: '018956' })
  assert.equal(country.value, 'SG')
  assert.equal(postal.value, '018956')
})

test('a form that changes after the fill is filled again each time it settles', () => {
  const env = page()
  const postal = env.field('postal-code')
  env.field('country', { value: '' })
  env.field('given-name')
  env.fill()
  assert.equal(postal.value, '10001')

  postal.value = ''
  const city = env.field('address-level2')
  env.mutate()
  env.later(199)
  assert.equal(city.value, '', 'not while the form is still changing')
  env.later(1)
  assert.equal(city.value, 'New York')
  assert.equal(postal.value, '10001', 'a field the page cleared is filled again')

  const late = env.field('family-name')
  env.mutate()
  env.later(200)
  assert.equal(late.value, 'Lovelace', 'and again at the next lull')

  env.later(2000)
  const after = env.field('organization')
  env.mutate()
  env.later(200)
  assert.equal(after.value, '', 'not after two seconds')
  assert.equal(env.watching(), false)
})

test('a list that loads after a loading state is filled when it arrives', () => {
  const env = page()
  env.field('postal-code')
  env.field('given-name')
  const region = env.list('address-level1', [['', 'Choose a country first']])
  env.list('country', [['', 'Choose'], ['US', 'United States']])
  env.fill()
  region.options = [{ value: '', text: 'Loading...' }]
  env.mutate()
  env.later(250)
  assert.equal(region.value, '')
  region.options = [['', 'Choose'], ['CA', 'California'], ['NY', 'New York']].map(([value, text]) => ({ value, text }))
  env.mutate()
  env.later(200)
  assert.equal(region.value, 'NY')
})

test('a fill whose field the country change replaced stops there', () => {
  const env = page()
  const postal = env.field('postal-code')
  const country = env.list('country', [['', 'Choose'], ['US', 'United States']])
  env.field('given-name')
  country.onchange = () => {
    postal.isConnected = false
  }
  env.fill()
  assert.equal(country.value, 'US')
  assert.equal(env.watching(), false)
})

test('picking another address replaces the last pick, but not what was typed since', () => {
  const env = page()
  const postal = env.field('postal-code')
  const given = env.field('given-name')
  const city = env.field('address-level2')
  env.fill()
  assert.deepEqual([postal.value, given.value, city.value], ['10001', 'Ada', 'New York'])
  city.value = 'Brooklyn'
  env.fill({ ...VALUES, given: 'Grace', postal: '20500', city: 'Washington' })
  assert.deepEqual([postal.value, given.value, city.value], ['20500', 'Grace', 'Brooklyn'])
})

test('the second pass leaves typed values, and stops on another page or a newer fill', () => {
  const typed = page()
  typed.field('postal-code')
  typed.field('given-name')
  typed.field('country')
  typed.fill()
  const city = typed.field('address-level2', { value: 'Paris' })
  typed.mutate()
  typed.later(200)
  assert.equal(city.value, 'Paris')

  const moved = page()
  moved.field('postal-code')
  moved.field('given-name')
  moved.field('country')
  moved.fill()
  const away = moved.field('address-level2')
  moved.location.origin = 'https://elsewhere.example'
  moved.mutate()
  moved.later(200)
  assert.equal(away.value, '')

  const newer = page()
  newer.field('postal-code')
  newer.field('given-name')
  newer.field('country')
  newer.fill()
  const stale = newer.field('address-level2')
  newer.window.__wisAddressFill += 1
  newer.mutate()
  newer.later(200)
  assert.equal(stale.value, '')
})

test('a form that does not change stops being watched after two seconds', () => {
  const env = page()
  env.field('postal-code')
  env.field('given-name')
  env.field('country')
  env.fill()
  env.later(2000)
  const city = env.field('address-level2')
  env.mutate()
  env.later(200)
  assert.equal(city.value, '')
})
