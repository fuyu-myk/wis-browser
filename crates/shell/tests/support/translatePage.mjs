// Runs the page translator in `translation/page/` over a fake document, with
// observers and timers the test drives by hand.

import { NodeFilter, eventTarget, handlers } from './s2-dom.mjs'
import { concatenated } from './rustSource.mjs'

const SCRIPT = concatenated('translation/page.rs', 'PAGE_JS')

/** An IntersectionObserver whose callbacks the test fires by hand. */
function observerClass() {
  const observers = []
  class FakeObserver {
    constructor(callback) {
      this.callback = callback
      this.targets = new Set()
      observers.push(this)
    }
    observe(target) {
      this.targets.add(target)
    }
    unobserve(target) {
      this.targets.delete(target)
    }
    disconnect() {
      this.targets.clear()
    }
    /** Report `targets` as scrolled into view. */
    show(...targets) {
      this.callback(targets.map((target) => ({ target, isIntersecting: true })))
    }
  }
  return { FakeObserver, observers }
}

/** A MutationObserver whose records the test hands over by hand. */
function mutationClass() {
  const mutations = []
  class FakeMutations {
    constructor(callback) {
      this.callback = callback
      this.watching = false
      mutations.push(this)
    }
    observe() {
      this.watching = true
    }
    disconnect() {
      this.watching = false
    }
    takeRecords() {
      return []
    }
    /** Report `records` as the page's changes. */
    fire(...records) {
      if (this.watching) this.callback(records)
    }
  }
  return { FakeMutations, mutations }
}

/**
 * Runs the script over `doc` and returns its API, what it posted, and the
 * observers. `findMain` stands in for the reader's main-content finder.
 */
export function load(doc, { findMain } = {}) {
  const { webkit, sent } = handlers('wisTranslate')
  const getComputedStyle = (el) => ({
    display: el.style.display || '',
    whiteSpace: el.style.whiteSpace || '',
  })
  const window = eventTarget({ webkit, getComputedStyle })
  const { FakeObserver, observers } = observerClass()
  const { FakeMutations, mutations } = mutationClass()
  const timers = []
  const setTimeout = (fn) => timers.push(fn)
  new Function(
    'window',
    'document',
    'NodeFilter',
    'IntersectionObserver',
    'setTimeout',
    'MutationObserver',
    '__wisFindMainContent',
    SCRIPT,
  )(window, doc, NodeFilter, FakeObserver, setTimeout, FakeMutations, findMain)
  const messages = () => sent.map(({ body }) => JSON.parse(body))
  const flush = () => {
    while (timers.length) timers.shift()()
  }
  const bodies = () => sent.map(({ body }) => body)
  return { api: window.__wisTranslate, window, messages, bodies, observers, mutations, flush }
}

/**
 * Starts and runs translation generation `gen` from `source`, with every root
 * on screen.
 */
export function goVisible(loaded, gen = 1, source = '', target = 'en') {
  loaded.api.start(gen)
  loaded.api.go(gen, source, target)
  const observer = loaded.observers.at(-1)
  observer.show(...observer.targets)
  loaded.flush()
  return loaded.messages().filter((m) => m.kind === 'blocks')
}

export const blocksOf = (messages) => messages.flatMap((m) => m.blocks)
