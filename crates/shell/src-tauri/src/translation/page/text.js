// Reading the page's text, shared by the probe and the translator.
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
const FOOTNOTE = /foot|note|ref|cite/i

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
const parentElementOf = getter(NodeType, 'parentElement')
const kind = getter(NodeType, 'nodeType')
const childNodesOf = getter(NodeType, 'childNodes')
const tagOf = getter(ElementType, 'tagName')
const matches = method(ElementType, 'matches')
const attributeOf = method(ElementType, 'getAttribute')
const closest = method(ElementType, 'closest')
const titleOf = getter(DocumentType, 'title')
const documentElementOf = getter(DocumentType, 'documentElement')
const childrenOf = (node) => Array.from(childNodesOf(node))

const worded = (text) => /\p{L}/u.test(text)

// Codes that name the same language here.
const ALIASES = new Map([
  ['iw', 'he'], ['in', 'id'], ['ji', 'yi'], ['jw', 'jv'], ['mo', 'ro'], ['tl', 'fil'],
  ['nb', 'no'], ['nn', 'no'], ['cmn', 'zh'], ['yue', 'zh'], ['wuu', 'zh'],
])
function primaryOf(lang) {
  const code = (lang || '').split(/[-_,\s]/)[0].toLowerCase()
  return ALIASES.get(code) || code
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

function marker(el) {
  if (tagOf(el) !== 'SUP' || worded(textOf(el))) return false
  const local = (a) => (attributeOf(a, 'href') || '').startsWith('#')
  const links = method(ElementType, 'querySelectorAll')(el, 'a')
  return Array.from(links).some(local) || FOOTNOTE.test(attributeOf(el, 'class') || '')
}
