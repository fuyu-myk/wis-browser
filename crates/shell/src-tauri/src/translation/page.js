// Page translation, in Wis's content world. `start` reports the page's
// declared language, `go` splits the page into segments and sends the ones on
// screen to `wisTranslate`, main content first, `apply` puts one translation
// in place and `revert` restores the page. Text is only ever set as text
// nodes, and the page's own nodes are never detached.
(function () {
  if (window.__wisTranslate) return

  const BLOCKS = new Set([
    'ADDRESS', 'ARTICLE', 'ASIDE', 'BLOCKQUOTE', 'BODY', 'BUTTON', 'CAPTION', 'CENTER', 'DD',
    'DETAILS', 'DIALOG', 'DIV', 'DL', 'DT', 'FIELDSET', 'FIGCAPTION', 'FIGURE', 'FOOTER', 'FORM',
    'H1', 'H2', 'H3', 'H4', 'H5', 'H6', 'HEADER', 'HGROUP', 'LABEL', 'LEGEND', 'LI', 'MAIN', 'NAV',
    'OL', 'P', 'SECTION', 'SUMMARY', 'TABLE', 'TBODY', 'TD', 'TFOOT', 'TH', 'THEAD', 'TR', 'UL',
  ])
  const SKIP =
    'script, style, noscript, template, code, pre, kbd, samp, var, textarea, input, select, ' +
    'option, svg, math, [contenteditable=""], [contenteditable=true], ' +
    '[contenteditable=plaintext-only], [translate=no], .notranslate'
  // Kept inside a sentence where they stand, their text left as it is.
  const OPAQUE = 'code, kbd, samp, var, [translate=no], .notranslate'
  // Messages carry at most this many segments; longer segments are left as they are.
  const BATCH = 24
  const MAX_SEGMENTS = 2000
  const MAX_CHARS = 4000
  const MAX_RUNS = 128
  const MAX_LANG = 64
  const MAX_KEYED = 256

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
  const kind = getter(NodeType, 'nodeType')
  const childNodesOf = getter(NodeType, 'childNodes')
  const tagOf = getter(ElementType, 'tagName')
  const matches = method(ElementType, 'matches')
  const attributeOf = method(ElementType, 'getAttribute')
  const closest = method(ElementType, 'closest')
  const insertBefore = method(NodeType, 'insertBefore')
  const removeChild = method(NodeType, 'removeChild')
  const bodyOf = getter(DocumentType, 'body')
  const documentElementOf = getter(DocumentType, 'documentElement')
  const createText = (text) => method(DocumentType, 'createTextNode')(document, text)
  const childrenOf = (node) => Array.from(childNodesOf(node))

  let state = null

  const blank = (text) => !/\S/.test(text)
  const worded = (text) => /\p{L}/u.test(text)

  function post(payload) {
    try {
      window.webkit.messageHandlers.wisTranslate.postMessage(JSON.stringify(payload))
    } catch (err) {}
  }

  function declaredLanguage() {
    const html = documentElementOf(document)
    const lang = html && (html.getAttribute('lang') || html.getAttribute('xml:lang'))
    if (lang) return lang.slice(0, MAX_LANG)
    for (const meta of method(DocumentType, 'querySelectorAll')(document, 'meta')) {
      if ((meta.getAttribute('http-equiv') || '').toLowerCase() === 'content-language') {
        return (meta.getAttribute('content') || '').slice(0, MAX_LANG)
      }
    }
    return ''
  }

  function mainContent() {
    try {
      return typeof __wisFindMainContent === 'function' ? __wisFindMainContent() : null
    } catch (err) {
      return null
    }
  }

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
    const lang = marked ? primaryOf(attributeOf(marked, 'lang')) : ''
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
      else if (type === 1 && !BLOCKS.has(tagOf(node)) && !matches(node, SKIP)) {
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
    const meaningful = (n) => kind(n) === 1 || !blank(n.nodeValue)
    let start = 0
    let end = nodes.length
    while (start < end && !meaningful(nodes[start])) start++
    while (end > start && !meaningful(nodes[end - 1])) end--
    return nodes.slice(start, end)
  }

  // Runs of a root's children that hold the block's text, code kept in place
  // among them. Anything else, a line break, an image, a nested block, ends a
  // segment and stays where it is.
  function groupsOf(root, assigned, carriers) {
    const groups = []
    let current = []
    const close = () => {
      const nodes = trimBlank(current)
      if (nodes.length) groups.push(nodes)
      current = []
    }
    for (const child of childrenOf(root)) {
      const type = kind(child)
      if (type === 3 && (assigned.has(child) || blank(child.nodeValue))) current.push(child)
      else if (type === 1 && (carriers.has(child) || opaque(child))) current.push(child)
      else if (type !== 8) close()
    }
    close()
    return groups
  }

  function segmentOf(root, nodes) {
    const elements = nodes.filter((n) => kind(n) === 1)
    const loose = nodes.filter((n) => kind(n) === 3 && !blank(n.nodeValue))
    if (elements.length === 1 && !loose.length && !opaque(elements[0])) {
      const text = ownText(elements[0])
      if (!worded(text)) return null
      return { root, nodes, whole: elements[0], tagged: [], opaque: new Set(), runs: [{ text }], sent: text }
    }
    const tagged = []
    const fixed = new Set()
    const runs = nodes.map((n) => {
      if (kind(n) === 3) return { text: n.nodeValue }
      tagged.push(n)
      const tag = tagged.length - 1
      if (opaque(n)) {
        fixed.add(tag)
        return { text: textOf(n), tag }
      }
      return { text: ownText(n), tag }
    })
    const translatable = runs.some((r) => (r.tag == null || !fixed.has(r.tag)) && worded(r.text))
    if (!translatable) return null
    return { root, nodes, whole: null, tagged, opaque: fixed, runs, sent: runs.map((r) => r.text).join('') }
  }

  // The segment's text now, to tell whether the page changed it since.
  function currentText(seg) {
    if (seg.whole) return ownText(seg.whole)
    return seg.nodes
      .map((n) => {
        if (kind(n) === 3) return n.nodeValue
        return seg.opaque.has(seg.tagged.indexOf(n)) ? textOf(n) : ownText(n)
      })
      .join('')
  }

  // The framework drops the tags of a segment with no text of its own.
  function partsOf(nodes) {
    if (nodes.length === 1 || nodes.some((n) => kind(n) === 3 && !blank(n.nodeValue))) return [nodes]
    return nodes.filter((n) => kind(n) === 1).map((n) => [n])
  }

  function collect() {
    const body = bodyOf(document)
    if (!body) return []
    const byBlock = new Map()
    const rejected = new Map()
    const reject = (el) => {
      if (!rejected.has(el)) rejected.set(el, !!closest(el, SKIP) || foreign(el, state.primary))
      return rejected.get(el)
    }
    const walker = method(DocumentType, 'createTreeWalker')(document, body, NodeFilter.SHOW_TEXT, {
      acceptNode(node) {
        const parent = parentElementOf(node)
        if (blank(node.nodeValue) || !parent || reject(parent)) return NodeFilter.FILTER_REJECT
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
        segment.id = segments.length
        segment.applied = null
        segment.key = segment.sent.length <= MAX_KEYED ? JSON.stringify([segment.runs, [...segment.opaque]]) : null
        segments.push(segment)
        if (segments.length >= MAX_SEGMENTS) return segments
      }
    }
    return segments
  }

  function sendable(s, seg) {
    if (seg.applied || (seg.key && s.sentKeys.has(seg.key))) return false
    if (seg.key) s.sentKeys.add(seg.key)
    return true
  }

  function flush(s) {
    if (s !== state) return
    s.scheduled = false
    const pending = s.pending.sort((a, b) => b.main - a.main || a.id - b.id).filter((seg) => sendable(s, seg))
    s.pending = []
    if (!pending.length) return
    s.wave++
    for (let i = 0; i < pending.length; i += BATCH) {
      const blocks = pending.slice(i, i + BATCH).map((seg) => ({ id: seg.id, runs: seg.runs, main: seg.main }))
      post({ kind: 'blocks', gen: s.gen, wave: s.wave, blocks })
    }
  }

  function queue(segments) {
    const s = state
    for (const seg of segments) s.pending.push(seg)
    if (s.scheduled) return
    s.scheduled = true
    setTimeout(() => flush(s), 0)
  }

  function seen(entries) {
    if (!state) return
    for (const entry of entries) {
      const segments = entry.isIntersecting ? state.byRoot.get(entry.target) : null
      if (!segments) continue
      state.byRoot.delete(entry.target)
      state.observer.unobserve(entry.target)
      queue(segments)
    }
  }

  function start(gen) {
    revert()
    state = {
      gen,
      primary: '',
      segments: null,
      byRoot: null,
      byKey: null,
      sentKeys: new Set(),
      observer: null,
      pending: [],
      scheduled: false,
      wave: 0,
    }
    post({ kind: 'start', gen, lang: declaredLanguage() })
  }

  function go(gen, source) {
    if (!state || state.gen !== gen || state.segments) return
    const main = mainContent()
    state.primary = primaryOf(source)
    state.segments = collect()
    state.byRoot = new Map()
    state.byKey = new Map()
    for (const seg of state.segments) {
      seg.main = !!main && within(main, seg.root)
      if (!state.byRoot.has(seg.root)) state.byRoot.set(seg.root, [])
      state.byRoot.get(seg.root).push(seg)
      if (!seg.key) continue
      if (!state.byKey.has(seg.key)) state.byKey.set(seg.key, [])
      state.byKey.get(seg.key).push(seg)
    }
    if (typeof IntersectionObserver !== 'function') {
      queue(state.segments)
      return
    }
    state.observer = new IntersectionObserver(seen, { rootMargin: '400px 0px' })
    for (const root of state.byRoot.keys()) state.observer.observe(root)
  }

  function remember(saved, t) {
    if (saved && !saved.has(t)) saved.set(t, t.nodeValue)
  }

  // Puts `text` in the element's first text node and empties the rest,
  // keeping any nested markup. Originals go into `saved` when given.
  function setText(el, text, saved) {
    textNodesIn(el).forEach((t, i) => {
      remember(saved, t)
      t.nodeValue = i === 0 ? text : ''
    })
  }

  function copyOf(el) {
    const copy = el.cloneNode(true)
    copy.removeAttribute('id')
    for (const inner of copy.querySelectorAll('[id]')) inner.removeAttribute('id')
    return copy
  }

  const textRuns = (runs) => Array.isArray(runs) && runs.every((r) => r && typeof r.text === 'string')

  function usable(runs, seg) {
    if (!textRuns(runs)) return false
    if (blank(runs.map((r) => r.text).join('')) && !blank(seg.sent)) return false
    const tags = new Set()
    for (const r of runs) {
      if (r.tag == null) continue
      if (!Number.isInteger(r.tag) || r.tag < 0 || r.tag >= seg.tagged.length) return false
      tags.add(r.tag)
    }
    return tags.size === seg.tagged.length
  }

  // The framework drops a segment's leading and trailing spaces, which keep
  // it apart from an image or a line break beside it.
  function withEdges(runs, sent) {
    const out = runs.slice()
    const text = runs.map((r) => r.text).join('')
    const lead = /^\s*/.exec(sent)[0]
    const trail = /\s*$/.exec(sent)[0]
    if (lead && !/^\s/.test(text)) {
      if (out[0] && out[0].tag == null) out[0] = { text: lead + out[0].text }
      else out.unshift({ text: lead, edge: true })
    }
    const last = out[out.length - 1]
    if (trail && !/\s$/.test(text)) {
      if (last && last.tag == null) out[out.length - 1] = { text: last.text + trail }
      else out.push({ text: trail, edge: true })
    }
    return out
  }

  // Lays `nodes` out in order where the segment's nodes stood. Every node is
  // already a child of `root` or new, so the page's nodes only move within it.
  function arrange(root, nodes, anchor) {
    const marker = createText('')
    insertBefore(root, marker, anchor)
    for (const n of nodes) insertBefore(root, n, marker)
    removeChild(root, marker)
  }

  const inOrder = (a, b) => a.length === b.length && a.every((n, i) => n === b[i])

  function apply(gen, id, translation) {
    if (!state || state.gen !== gen || !state.segments) return
    const seg = state.segments[id]
    if (!seg) return
    for (const each of seg.key ? state.byKey.get(seg.key) : [seg]) put(each, translation)
  }

  function put(seg, translation) {
    if (seg.applied || !usable(translation, seg) || currentText(seg) !== seg.sent) return
    const runs = withEdges(translation, seg.sent)
    const saved = new Map()
    if (seg.whole) {
      setText(seg.whole, runs.map((r) => r.text).join(''), saved)
      seg.applied = { created: [], saved }
      return
    }
    if (seg.nodes.some((n) => parentOf(n) !== seg.root)) return
    const loose = seg.nodes.filter((n) => kind(n) === 3)
    const created = []
    const used = new Set()
    let next = 0
    const laid = []
    for (const run of runs) {
      if (run.tag == null) {
        let t = run.edge ? null : loose[next++]
        if (t) {
          remember(saved, t)
          t.nodeValue = run.text
        } else {
          t = createText(run.text)
          created.push(t)
        }
        laid.push(t)
        continue
      }
      const el = seg.tagged[run.tag]
      if (!used.has(run.tag)) {
        used.add(run.tag)
        if (!seg.opaque.has(run.tag)) setText(el, run.text, saved)
        laid.push(el)
      } else if (!seg.opaque.has(run.tag)) {
        const copy = copyOf(el)
        setText(copy, run.text, null)
        created.push(copy)
        laid.push(copy)
      }
    }
    for (; next < loose.length; next++) {
      remember(saved, loose[next])
      loose[next].nodeValue = ''
      laid.push(loose[next])
    }
    if (!inOrder(laid, seg.nodes)) arrange(seg.root, laid, seg.nodes[0])
    seg.applied = { created, saved }
  }

  function restore(seg) {
    const { created, saved } = seg.applied
    for (const n of created) if (parentOf(n) === seg.root) removeChild(seg.root, n)
    if (!seg.whole && seg.nodes.every((n) => parentOf(n) === seg.root)) {
      const members = new Set(seg.nodes)
      const now = childrenOf(seg.root).filter((n) => members.has(n))
      if (!inOrder(now, seg.nodes)) arrange(seg.root, seg.nodes, now[0])
    }
    for (const [t, value] of saved) t.nodeValue = value
    seg.applied = null
  }

  function revert() {
    if (!state) return
    if (state.observer) state.observer.disconnect()
    for (const seg of state.segments || []) if (seg.applied) restore(seg)
    state = null
  }

  window.addEventListener('pagehide', (event) => {
    if (event.persisted) revert()
  })

  window.__wisTranslate = { start, go, apply, revert }
})()
