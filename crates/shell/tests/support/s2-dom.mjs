// A small hand-written DOM for the page-script tests: elements and text nodes,
// the selector subset the scripts use, listener registries, and the find
// engine's tree walker, range and highlight fakes. Only what the scripts touch.

const VOID = new Set(['AREA', 'BR', 'HR', 'IMG', 'INPUT', 'LINK', 'META', 'SOURCE', 'WBR'])
const LINKS = new Set(['A', 'AREA'])

export const NodeFilter = { SHOW_TEXT: 4, FILTER_ACCEPT: 1, FILTER_REJECT: 2, FILTER_SKIP: 3 }

function detach(node) {
  const parent = node.parentNode
  if (!parent) return
  parent.childNodes = parent.childNodes.filter((n) => n !== node)
  node.parentNode = null
}

function rootOf(node) {
  let n = node
  while (n.parentNode) n = n.parentNode
  return n
}

export class Text {
  constructor(value) {
    this.nodeType = 3
    this.nodeValue = value
    this.parentNode = null
  }
  get parentElement() {
    return this.parentNode && this.parentNode.nodeType === 1 ? this.parentNode : null
  }
  get textContent() {
    return this.nodeValue
  }
  cloneNode() {
    return new Text(this.nodeValue)
  }
  remove() {
    detach(this)
  }
}

class Style {
  constructor() {
    this.cssText = ''
    this.priorities = {}
  }
  setProperty(name, value, priority = '') {
    this[name] = value
    this.priorities[name] = priority
  }
}

export class Element {
  constructor(tag, attrs = {}) {
    this.nodeType = 1
    this.tagName = tag.toUpperCase()
    this.attrMap = new Map(Object.entries(attrs).map(([k, v]) => [k, String(v)]))
    this.childNodes = []
    this.parentNode = null
    this.style = new Style()
    this.rect = { left: 0, top: 0, right: 0, bottom: 0, width: 0, height: 0 }
    this.scrolls = []
  }
  get id() {
    return this.attrMap.get('id') ?? ''
  }
  set id(value) {
    this.attrMap.set('id', String(value))
  }
  get className() {
    return this.attrMap.get('class') ?? ''
  }
  get classList() {
    return (this.getAttribute('class') || '').split(/\s+/).filter(Boolean)
  }
  get attributes() {
    return [...this.attrMap].map(([name, value]) => ({ name, value }))
  }
  getAttribute(name) {
    return this.attrMap.has(name) ? this.attrMap.get(name) : null
  }
  setAttribute(name, value) {
    this.attrMap.set(name, String(value))
  }
  hasAttribute(name) {
    return this.attrMap.has(name)
  }
  removeAttribute(name) {
    this.attrMap.delete(name)
  }
  get href() {
    if (!LINKS.has(this.tagName)) return undefined
    const raw = this.getAttribute('href')
    if (raw == null) return ''
    const root = rootOf(this)
    const base = root.nodeType === 9 ? root.location.href : 'https://page.test/'
    try {
      return new URL(raw, base).href
    } catch {
      return raw
    }
  }
  get children() {
    return this.childNodes.filter((n) => n.nodeType === 1)
  }
  get parentElement() {
    return this.parentNode && this.parentNode.nodeType === 1 ? this.parentNode : null
  }
  get textContent() {
    return this.childNodes.map((n) => n.textContent).join('')
  }
  set textContent(value) {
    this.childNodes.forEach((n) => (n.parentNode = null))
    this.childNodes = []
    if (value) this.appendChild(new Text(String(value)))
  }
  get innerText() {
    return this.textContent
  }
  get isConnected() {
    return rootOf(this).nodeType === 9
  }
  get innerHTML() {
    return this.childNodes.map(serialize).join('')
  }
  // A page enforcing Trusted Types refuses an HTML string.
  set innerHTML(html) {
    throw new TypeError("This document requires 'TrustedHTML' assignment.")
  }
  get firstChild() {
    return this.childNodes[0] ?? null
  }
  set className(value) {
    this.setAttribute('class', value)
  }
  replaceChildren(...nodes) {
    for (const node of [...this.childNodes]) node.parentNode = null
    this.childNodes = []
    nodes.forEach((node) => this.appendChild(node))
  }
  appendChild(node) {
    detach(node)
    node.parentNode = this
    this.childNodes.push(node)
    return node
  }
  remove() {
    detach(this)
  }
  cloneNode(deep) {
    const copy = new Element(this.tagName, Object.fromEntries(this.attrMap))
    if (deep) this.childNodes.forEach((n) => copy.appendChild(n.cloneNode(true)))
    return copy
  }
  matches(selector) {
    return parseList(selector).some((s) => matchComplex(this, s))
  }
  closest(selector) {
    const list = parseList(selector)
    for (let n = this; n; n = n.parentElement) if (list.some((s) => matchComplex(n, s))) return n
    return null
  }
  querySelectorAll(selector) {
    const list = parseList(selector)
    return descendants(this).filter((e) => list.some((s) => matchComplex(e, s)))
  }
  querySelector(selector) {
    return this.querySelectorAll(selector)[0] ?? null
  }
  getBoundingClientRect() {
    return this.rect
  }
  scrollIntoView(options) {
    this.scrolls.push(options)
  }
}

/** Build an element: `h('div', { class: 'a' }, 'text', h('p'))`. */
export function h(tag, attrs = {}, ...children) {
  const el = new Element(tag, attrs)
  for (const child of children.flat()) el.appendChild(typeof child === 'string' ? new Text(child) : child)
  return el
}

function descendants(el) {
  const out = []
  const walk = (node) => {
    for (const child of node.children) {
      out.push(child)
      walk(child)
    }
  }
  walk(el)
  return out
}

function textNodesUnder(node) {
  const out = []
  const walk = (n) => {
    for (const child of n.childNodes) {
      if (child.nodeType === 3) out.push(child)
      else walk(child)
    }
  }
  walk(node)
  return out
}

function escapeText(text) {
  return text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
}

function serialize(node) {
  if (node.nodeType === 3) return escapeText(node.nodeValue)
  const tag = node.tagName.toLowerCase()
  const attrs = node.attributes
    .map((a) => ` ${a.name}="${a.value.replace(/&/g, '&amp;').replace(/"/g, '&quot;')}"`)
    .join('')
  if (VOID.has(node.tagName)) return `<${tag}${attrs}>`
  return `<${tag}${attrs}>${node.innerHTML}</${tag}>`
}

// Selectors: comma lists of compounds joined by descendant or child
// combinators. A compound is tag or *, #id, .class, [attr], [attr=value] and
// :nth-of-type(n). Anything else throws, as querySelectorAll does.

const TOKEN =
  /([a-zA-Z][\w-]*|\*)|#([\w-]+)|\.([\w-]+)|\[([\w-]+)(?:=("[^"]*"|'[^']*'|[^\]]*))?\]|:nth-of-type\((\d+)\)/y

function parseCompound(src) {
  const compound = { tag: null, ids: [], classes: [], attrs: [], nth: null }
  let at = 0
  while (at < src.length) {
    TOKEN.lastIndex = at
    const m = TOKEN.exec(src)
    if (!m) throw new SyntaxError(`unsupported selector: ${src}`)
    if (m[1]) {
      if (at !== 0) throw new SyntaxError(`unsupported selector: ${src}`)
      compound.tag = m[1]
    } else if (m[2]) compound.ids.push(m[2])
    else if (m[3]) compound.classes.push(m[3])
    else if (m[4]) compound.attrs.push([m[4], m[5] === undefined ? undefined : m[5].replace(/^["']|["']$/g, '')])
    else compound.nth = Number(m[6])
    at = TOKEN.lastIndex
  }
  return compound
}

function splitList(selector) {
  const out = []
  let depth = 0
  let current = ''
  for (const ch of selector) {
    if (ch === '[' || ch === '(') depth++
    if (ch === ']' || ch === ')') depth--
    if (ch === ',' && depth === 0) {
      out.push(current)
      current = ''
    } else current += ch
  }
  out.push(current)
  return out.map((s) => s.trim())
}

function parseComplex(src) {
  const tokens = src.replace(/\s*>\s*/g, ' > ').trim().split(/\s+/)
  const parts = []
  let combinator = ' '
  for (const token of tokens) {
    if (token === '>') {
      combinator = '>'
      continue
    }
    parts.push({ combinator: parts.length ? combinator : null, compound: parseCompound(token) })
    combinator = ' '
  }
  if (!parts.length) throw new SyntaxError(`empty selector: ${src}`)
  return parts
}

function parseList(selector) {
  return splitList(String(selector)).map(parseComplex)
}

function nthOfType(el) {
  const parent = el.parentElement
  if (!parent) return 1
  return parent.children.filter((c) => c.tagName === el.tagName).indexOf(el) + 1
}

function matchCompound(el, c) {
  if (c.tag && c.tag !== '*' && el.tagName !== c.tag.toUpperCase()) return false
  if (c.ids.some((id) => el.id !== id)) return false
  const classes = el.classList
  if (c.classes.some((k) => !classes.includes(k))) return false
  if (c.attrs.some(([n, v]) => !el.hasAttribute(n) || (v !== undefined && el.getAttribute(n) !== v))) return false
  if (c.nth !== null && nthOfType(el) !== c.nth) return false
  return true
}

function matchComplex(el, parts) {
  const at = (node, i) => {
    if (!matchCompound(node, parts[i].compound)) return false
    if (i === 0) return true
    if (parts[i].combinator === '>') return Boolean(node.parentElement) && at(node.parentElement, i - 1)
    for (let p = node.parentElement; p; p = p.parentElement) if (at(p, i - 1)) return true
    return false
  }
  return at(el, parts.length - 1)
}

/** Listener registry with `fire`, mixed into the document and windows. */
export function eventTarget(target = {}) {
  const map = new Map()
  target.addEventListener = (type, fn, capture) => {
    const list = map.get(type) || []
    map.set(type, [...list, { fn, capture: Boolean(capture) }])
  }
  target.removeEventListener = (type, fn, capture) => {
    const list = map.get(type) || []
    map.set(
      type,
      list.filter((l) => !(l.fn === fn && l.capture === Boolean(capture))),
    )
  }
  target.listenerCount = (type) => (map.get(type) || []).length
  target.fire = (type, props = {}) => {
    const event = makeEvent(type, props)
    for (const l of [...(map.get(type) || [])]) l.fn.call(target, event)
    return event
  }
  return target
}

/** A mouse/keyboard-ish event that records preventDefault and stopPropagation. */
export function makeEvent(type, props = {}) {
  return {
    type,
    button: 0,
    altKey: false,
    metaKey: false,
    ctrlKey: false,
    shiftKey: false,
    isTrusted: true,
    clientX: 0,
    clientY: 0,
    ...props,
    defaultPrevented: false,
    propagationStopped: false,
    preventDefault() {
      this.defaultPrevented = true
    },
    stopPropagation() {
      this.propagationStopped = true
    },
  }
}

export function makeLocation(url) {
  const u = new URL(url)
  return {
    href: u.href,
    origin: u.origin,
    protocol: u.protocol,
    host: u.host,
    hostname: u.hostname,
    pathname: u.pathname,
    search: u.search,
    hash: u.hash,
  }
}

class TreeWalker {
  constructor(root, filter) {
    this.pending = textNodesUnder(root)
    this.filter = filter
  }
  nextNode() {
    while (this.pending.length) {
      const node = this.pending.shift()
      if (this.filter.acceptNode(node) === NodeFilter.FILTER_ACCEPT) return node
    }
    return null
  }
}

/** A single-text-node range, enough for the find engine. */
export class Range {
  setStart(node, offset) {
    if (offset > node.nodeValue.length) throw new RangeError('IndexSizeError')
    this.startContainer = node
    this.startOffset = offset
  }
  setEnd(node, offset) {
    if (offset > node.nodeValue.length) throw new RangeError('IndexSizeError')
    this.endContainer = node
    this.endOffset = offset
  }
  toString() {
    return this.startContainer.nodeValue.slice(this.startOffset, this.endOffset)
  }
  getBoundingClientRect() {
    return this.startContainer.parentElement.rect
  }
}

export class Highlight {
  constructor(...ranges) {
    this.ranges = ranges
  }
  add(range) {
    this.ranges.push(range)
  }
}

export class Document {
  constructor(url = 'https://page.test/') {
    eventTarget(this)
    this.nodeType = 9
    this.parentNode = null
    this.location = makeLocation(url)
    this.title = ''
    this.pointTarget = null
    const html = new Element('html')
    html.parentNode = this
    this.childNodes = [html]
    this.documentElement = html
    this.head = html.appendChild(new Element('head'))
    this.body = html.appendChild(new Element('body'))
  }
  createElement(tag) {
    return new Element(tag)
  }
  createTextNode(text) {
    return new Text(text)
  }
  querySelectorAll(selector) {
    const list = parseList(selector)
    const all = [this.documentElement, ...descendants(this.documentElement)]
    return all.filter((e) => list.some((s) => matchComplex(e, s)))
  }
  querySelector(selector) {
    return this.querySelectorAll(selector)[0] ?? null
  }
  getElementById(id) {
    return this.querySelectorAll('*').find((e) => e.id === id) ?? null
  }
  elementFromPoint() {
    return this.pointTarget
  }
  createTreeWalker(root, _whatToShow, filter) {
    return new TreeWalker(root, filter)
  }
  createRange() {
    return new Range()
  }
}

/** A document whose body holds `children`. */
export function page(url, ...children) {
  const doc = new Document(url)
  for (const child of children.flat()) doc.body.appendChild(typeof child === 'string' ? new Text(child) : child)
  return doc
}

/** `window.webkit.messageHandlers` with each named handler recording `{ name, body }` into `sent`. */
export function handlers(...names) {
  const sent = []
  const messageHandlers = {}
  for (const name of names) messageHandlers[name] = { postMessage: (body) => sent.push({ name, body }) }
  return { webkit: { messageHandlers }, sent }
}
