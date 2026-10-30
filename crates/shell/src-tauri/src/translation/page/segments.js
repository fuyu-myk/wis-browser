// The page read into segments: runs of a block's text, with the inline
// elements they sit in tagged and the ones a translation must keep marked.
const BLOCKS = new Set([
  'ADDRESS', 'ARTICLE', 'ASIDE', 'BLOCKQUOTE', 'BODY', 'BUTTON', 'CAPTION', 'CENTER', 'DD',
  'DETAILS', 'DIALOG', 'DIV', 'DL', 'DT', 'FIELDSET', 'FIGCAPTION', 'FIGURE', 'FOOTER', 'FORM',
  'H1', 'H2', 'H3', 'H4', 'H5', 'H6', 'HEADER', 'HGROUP', 'LABEL', 'LEGEND', 'LI', 'MAIN', 'NAV',
  'OL', 'P', 'SECTION', 'SUMMARY', 'TABLE', 'TBODY', 'TD', 'TFOOT', 'TH', 'THEAD', 'TR', 'UL',
])
const SKIP =
  'script, style, noscript, template, code, pre, kbd, samp, var, textarea, input, select, ' +
  'option, svg, math, rt, rp, [contenteditable=""], [contenteditable=true], ' +
  '[contenteditable=plaintext-only], [translate=no], .notranslate'
// Kept inside a sentence where they stand, their text left as it is.
const OPAQUE = 'code, kbd, samp, var, [translate=no], .notranslate'
// Inline elements a translation must keep; styling ones may be dropped, and
// so may a link in a sentence too long for Rust to repair.
const CONTROLS = '[role], [tabindex], [onclick]'
const NEEDED = 'a, ' + CONTROLS
// Elements that end a sentence rather than sit inside one, but for objects.
const BREAKS = new Set([
  'BR', 'HR', 'IMG', 'PICTURE', 'SVG', 'VIDEO', 'AUDIO', 'CANVAS', 'IFRAME', 'OBJECT', 'EMBED',
  'INPUT', 'SELECT', 'TEXTAREA', 'BUTTON', 'MATH', 'SCRIPT', 'STYLE', 'NOSCRIPT', 'TEMPLATE',
  'PROGRESS', 'METER', 'SLOT', 'LINK',
])

const OBJECTS = 'img, picture, svg, math, canvas'
const OBJECT = '\uFFFC'
const FOOTNOTE = /foot|note|ref|cite/i
const SENTENCE_END = /[。！？.!?]\s*$/
const BREAKING = [...BREAKS].join(', ').toLowerCase()

const MAX_SEGMENTS = 10000
const MAX_CHARS = 4000
const MAX_RUNS = 128
const MAX_IDS = 1000000
const MAX_PIECES = 12

// DOM reads through the prototypes: a page's named controls and images
// (an input named `parentNode`, an img named `body`) shadow the properties
// of their form or of the document, even in this world.
const typeOf = (name) => (typeof window[name] === 'function' ? window[name] : null)
function getter(type, name) {
  const d = type ? Object.getOwnPropertyDescriptor(type.prototype, name) : null
  return d && d.get ? (o) => d.get.call(o) : (o) => o[name]
}
function method(type, name) {
  const f = type ? type.prototype[name] : null
  return typeof f === 'function' ? (o, ...args) => f.apply(o, args) : (o, ...args) => o[name](...args)
}
const NodeType = typeOf('Node')
const ElementType = typeOf('Element')
const DocumentType = typeOf('Document')
const parentOf = getter(NodeType, 'parentNode')
const parentElementOf = getter(NodeType, 'parentElement')
const previousOf = getter(NodeType, 'previousSibling')
const kind = getter(NodeType, 'nodeType')
const childNodesOf = getter(NodeType, 'childNodes')
const tagOf = getter(ElementType, 'tagName')
const matches = method(ElementType, 'matches')
const querySelector = method(ElementType, 'querySelector')
const attributeOf = method(ElementType, 'getAttribute')
const closest = method(ElementType, 'closest')
const insertBefore = method(NodeType, 'insertBefore')
const removeChild = method(NodeType, 'removeChild')
const connected = getter(NodeType, 'isConnected')
const bodyOf = getter(DocumentType, 'body')
const titleOf = getter(DocumentType, 'title')
const documentElementOf = getter(DocumentType, 'documentElement')
const createText = (text) => method(DocumentType, 'createTextNode')(document, text)
const childrenOf = (node) => Array.from(childNodesOf(node))

function displayOf(el) {
  try {
    const style = typeof window.getComputedStyle === 'function' ? window.getComputedStyle(el) : null
    return style ? style.display : ''
  } catch (err) {
    return ''
  }
}

const blank = (text) => !/\S/.test(text)
const worded = (text) => /\p{L}/u.test(text)

function within(ancestor, node) {
  for (let n = node; n; n = parentOf(n)) if (n === ancestor) return true
  return false
}

// Codes that name the same language here, and codes that name none.
const ALIASES = new Map([
  ['iw', 'he'], ['in', 'id'], ['ji', 'yi'], ['jw', 'jv'], ['mo', 'ro'], ['tl', 'fil'],
  ['nb', 'no'], ['nn', 'no'], ['cmn', 'zh'], ['yue', 'zh'], ['wuu', 'zh'],
])
const UNMARKED = new Set(['', 'und', 'mul', 'x', 'i'])
function primaryOf(lang) {
  const code = (lang || '').split(/[-_,\s]/)[0].toLowerCase()
  return ALIASES.get(code) || code
}

function foreign(el, primary) {
  const marked = primary ? closest(el, '[lang]') : null
  const lang = marked && marked !== documentElementOf(document) ? primaryOf(attributeOf(marked, 'lang')) : ''
  return !UNMARKED.has(lang) && lang !== primary
}

function blockOf(el) {
  for (let e = el; e; e = parentElementOf(e)) if (BLOCKS.has(tagOf(e))) return e
  return bodyOf(document)
}

function childOf(root, node) {
  let n = node
  while (n && parentOf(n) !== root) n = parentOf(n)
  return n
}

// The deepest element holding all of a block's text.
function rootOf(block, texts) {
  let root = block
  for (;;) {
    const holders = new Set(texts.map((t) => childOf(root, t)))
    const only = holders.size === 1 ? [...holders][0] : null
    if (!only || kind(only) !== 1) return root
    root = only
  }
}

// An inline element's own text: a nested block's text is its own segment's.
function textNodesIn(el) {
  const out = []
  const stack = childrenOf(el).reverse()
  while (stack.length) {
    const node = stack.pop()
    const type = kind(node)
    if (type === 3) out.push(node)
    else if (type === 1 && !BLOCKS.has(tagOf(node)) && !matches(node, SKIP) && tagOf(node) !== 'WBR' && !marker(node)) {
      const kids = childrenOf(node)
      for (let i = kids.length - 1; i >= 0; i--) stack.push(kids[i])
    }
  }
  return out
}

function textOf(el) {
  let text = ''
  const stack = childrenOf(el).reverse()
  while (stack.length) {
    const node = stack.pop()
    if (kind(node) === 3) text += node.nodeValue
    else if (kind(node) === 1) {
      const kids = childrenOf(node)
      for (let i = kids.length - 1; i >= 0; i--) stack.push(kids[i])
    }
  }
  return text
}

const opaque = (el) =>
  !BLOCKS.has(tagOf(el)) &&
  (matches(el, OPAQUE) || foreign(el, state.primary)) &&
  !blank(textOf(el))
const ownText = (el) => textNodesIn(el).map((t) => t.nodeValue).join('')

function trimBlank(nodes) {
  const meaningful = (n) => (kind(n) === 1 ? !invisible(n) && !object(n) : kind(n) === 3 && !blank(n.nodeValue))
  let start = 0
  let end = nodes.length
  while (start < end && !meaningful(nodes[start])) start++
  while (end > start && !meaningful(nodes[end - 1])) end--
  return nodes.slice(start, end)
}

// A word-break hint, an empty element, a footnote number or a link to no
// stylesheet sits inside a sentence without being read.
function invisible(el) {
  const tag = tagOf(el)
  if (tag === 'WBR' || marker(el)) return true
  if (tag === 'LINK') return !/stylesheet/i.test(attributeOf(el, 'rel') || '')
  if (BLOCKS.has(tag) || BREAKS.has(tag) || tag.includes('-') || matches(el, OBJECTS) || !blank(textOf(el))) {
    return false
  }
  return !querySelector(el, BREAKING)
}

// An object, or an element holding only objects, keeps its place in the
// translation.
function object(el) {
  if (matches(el, OBJECTS)) return true
  const tag = tagOf(el)
  if (BLOCKS.has(tag) || BREAKS.has(tag)) return false
  if (!tag.includes('-') && !querySelector(el, OBJECTS)) return false
  return blank(ownText(el))
}

function marker(el) {
  if (tagOf(el) !== 'SUP' || worded(textOf(el))) return false
  const local = (a) => (attributeOf(a, 'href') || '').startsWith('#')
  const links = method(ElementType, 'querySelectorAll')(el, 'a')
  return Array.from(links).some(local) || FOOTNOTE.test(attributeOf(el, 'class') || '')
}

// A link or control inside an element makes it one the translation must keep.
function needs(el, selector = NEEDED) {
  if (opaque(el) || matches(el, selector)) return true
  for (const inner of method(ElementType, 'querySelectorAll')(el, selector)) {
    const sup = closest(inner, 'sup')
    if (!sup || !marker(sup)) return true
  }
  return false
}

// Runs of a root's children that hold the block's text, code and silent
// elements kept in place among them. Anything else ends a segment.
function groupsOf(root, assigned, carriers) {
  const groups = []
  let current = []
  const member = (el) => carriers.has(el) || opaque(el) || invisible(el) || object(el)
  const close = () => {
    let nodes = trimBlank(current)
    const lead = current[current.indexOf(nodes[0]) - 1]
    const text = nodes.map((n) => (kind(n) === 3 ? n.nodeValue : ownText(n))).join('')
    if (lead && kind(lead) === 1 && object(lead) && /^\S/.test(text) && SENTENCE_END.test(text)) {
      nodes = [lead, ...nodes]
    }
    if (nodes.length) groups.push(nodes)
    current = []
  }
  for (const child of childrenOf(root)) {
    const type = kind(child)
    if (type === 3 && (assigned.has(child) || blank(child.nodeValue))) current.push(child)
    else if ((type === 1 && member(child)) || type === 8) current.push(child)
    else close()
  }
  close()
  return groups
}

const textual = (n) => kind(n) === 3 || (kind(n) === 1 && !invisible(n))

function segmentOf(root, nodes) {
  const elements = nodes.filter((n) => kind(n) === 1 && !invisible(n))
  const loose = nodes.filter((n) => kind(n) === 3 && !blank(n.nodeValue))
  if (elements.length === 1 && !loose.length && !opaque(elements[0])) {
    const text = ownText(elements[0])
    if (!worded(text)) return null
    const none = { tagged: [], opaque: new Set(), needed: [], required: [], objects: [] }
    return { root, nodes, whole: elements[0], ...none, runs: [{ text }], sent: text }
  }
  const tagged = []
  const fixed = new Set()
  const needed = []
  const required = []
  const objects = []
  const runs = nodes.filter(textual).map((n) => {
    if (kind(n) === 3) return { text: n.nodeValue }
    tagged.push(n)
    const tag = tagged.length - 1
    const placeholder = object(n)
    if (placeholder || opaque(n)) {
      fixed.add(tag)
      needed.push(tag)
      required.push(tag)
      if (placeholder) objects.push(tag)
      return { text: placeholder ? OBJECT : textOf(n), tag }
    }
    if (needs(n)) needed.push(tag)
    if (needs(n, CONTROLS)) required.push(tag)
    return { text: ownText(n), tag }
  })
  const translatable = runs.some((r) => (r.tag == null || !fixed.has(r.tag)) && worded(r.text))
  if (!translatable) return null
  const sent = runs.map((r) => r.text).join('')
  const hidden = nodes.filter((n) => !textual(n))
  const long = runs.filter((r) => !blank(r.text)).length > MAX_PIECES
  return { root, nodes, whole: null, tagged, opaque: fixed, needed, required, long, objects, hidden, runs, sent }
}

// The segment's text now, to tell whether the page changed it since.
function currentText(seg) {
  if (seg.whole) return ownText(seg.whole)
  return seg.nodes
    .filter(textual)
    .map((n) => {
      if (kind(n) === 3) return n.nodeValue
      const tag = seg.tagged.indexOf(n)
      if (seg.objects.includes(tag)) return OBJECT
      return seg.opaque.has(tag) ? textOf(n) : ownText(n)
    })
    .join('')
}

function textsOf(seg) {
  const out = []
  for (const n of seg.whole ? [seg.whole] : seg.nodes) {
    if (kind(n) === 3) out.push(n)
    else for (const t of textNodesIn(n)) out.push(t)
  }
  return out
}

// The framework drops the tags of a segment with no text of its own.
// Links and controls go alone; the styling elements between them stay
// together when they make a sentence, and go one by one otherwise.
function partsOf(nodes) {
  if (nodes.length === 1 || nodes.some((n) => kind(n) === 3 && !blank(n.nodeValue))) return [nodes]
  const parts = []
  let styling = []
  const close = () => {
    const elements = styling.filter((n) => kind(n) === 1 && !invisible(n))
    if (SENTENCE_END.test(elements.map(textOf).join(''))) parts.push(trimBlank(styling))
    else for (const el of elements) parts.push([el])
    styling = []
  }
  for (const n of nodes) {
    if (kind(n) !== 1 || !needs(n)) {
      styling.push(n)
      continue
    }
    close()
    parts.push([n])
  }
  close()
  return parts
}

// New segments under `scope`, leaving text other segments already hold.
function collect(scope) {
  const room = Math.min(MAX_SEGMENTS - state.segments.size, MAX_IDS - state.nextId)
  if (!scope || room <= 0) return []
  const byBlock = new Map()
  const rejected = new Map()
  const reject = (el) => {
    if (!rejected.has(el)) {
      const sup = closest(el, 'sup')
      rejected.set(el, !!closest(el, SKIP) || foreign(el, state.primary) || (!!sup && marker(sup)))
    }
    return rejected.get(el)
  }
  const walker = method(DocumentType, 'createTreeWalker')(document, scope, NodeFilter.SHOW_TEXT, {
    acceptNode(node) {
      const parent = parentElementOf(node)
      if (blank(node.nodeValue) || !parent || state.owner.has(node) || state.given.has(node) || reject(parent)) {
        return NodeFilter.FILTER_REJECT
      }
      return NodeFilter.FILTER_ACCEPT
    },
  })
  for (let t = walker.nextNode(); t; t = walker.nextNode()) {
    const block = blockOf(parentElementOf(t))
    if (!byBlock.has(block)) byBlock.set(block, [])
    byBlock.get(block).push(t)
  }
  const segments = []
  for (const [block, texts] of byBlock) {
    const root = rootOf(block, texts)
    const carriers = new Set()
    for (const t of texts) {
      for (let e = parentElementOf(t); e && e !== root; e = parentElementOf(e)) carriers.add(e)
    }
    for (const nodes of groupsOf(root, new Set(texts), carriers).flatMap(partsOf)) {
      const segment = segmentOf(root, nodes)
      if (!segment || segment.sent.length > MAX_CHARS || segment.runs.length > MAX_RUNS) continue
      segment.texts = textsOf(segment)
      if (segment.texts.some((t) => state.owner.has(t))) continue
      segment.sig = JSON.stringify([segment.runs.map((r) => [r.text.length, r.tag]), [...segment.opaque], segment.needed])
      segments.push(segment)
      if (segments.length >= room) return segments
    }
  }
  return segments
}
