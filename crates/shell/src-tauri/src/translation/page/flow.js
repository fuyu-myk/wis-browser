// Page translation, in Wis's content world. `start` reports the page's
// declared language, `go` splits the page into segments and sends the ones on
// screen to `wisTranslate`, main content first, then watches for text the
// page adds or changes, `apply` puts one translation in place and `revert`
// restores the page. Text is only ever set as text nodes, and the page's
// own nodes are never detached.

const UNSPACED = new Set(['ja', 'zh', 'yue'])
// Messages carry at most this many segments.
const BATCH = 24
const MAX_LANG = 64
const MAX_KEPT = 2000000
const RESCAN_DELAY = 300
const MAX_RESCAN_DELAY = 5000
const SLOW_RESCAN = 20
const MAX_CHURN = 3
const CHURN_WINDOW = 10000

let state = null
let kept = { pair: '', translations: new Map(), chars: 0 }

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

// Segments match when their text, its runs and what stays untranslated
// do. Keyed by the text the segment already holds.
function among(map, seg, make) {
  let list = map.get(seg.sent)
  if (!list && !make) return undefined
  if (!list) map.set(seg.sent, (list = []))
  let found = list.find((entry) => entry.sig === seg.sig)
  if (!found && make) list.push((found = make()))
  return found
}

function sendable(seg) {
  if (seg.applied || seg.dead || seg.group.sent) return false
  seg.group.sent = true
  state.inflight.set(seg.id, seg)
  return true
}

function flush(s) {
  if (s !== state) return
  s.scheduled = false
  const pending = s.pending.sort((a, b) => b.main - a.main || a.id - b.id).filter(sendable)
  s.pending = []
  if (!pending.length) return
  s.wave++
  for (let i = 0; i < pending.length; i += BATCH) {
    const blocks = pending
      .slice(i, i + BATCH)
      .map((seg) => ({
        id: seg.id,
        runs: seg.runs,
        main: seg.main,
        needed: seg.needed,
        objects: seg.objects,
        long: !!seg.long,
      }))
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
    const segments = entry.isIntersecting ? state.byWatched.get(entry.target) : null
    if (!segments) continue
    state.byWatched.delete(entry.target)
    state.observer.unobserve(entry.target)
    queue(segments)
  }
}

function start(gen) {
  revert()
  state = {
    gen,
    primary: '',
    main: null,
    segments: null,
    nextId: 0,
    inflight: new Map(),
    byWatched: new Map(),
    groups: new Map(),
    owner: new WeakMap(),
    churn: new WeakMap(),
    given: new WeakSet(),
    observer: null,
    mutations: null,
    dirty: new Set(),
    rescan: false,
    delay: RESCAN_DELAY,
    pending: [],
    scheduled: false,
    wave: 0,
  }
  post({ kind: 'start', gen, lang: declaredLanguage() })
}

// An element shown only as its contents has no box for the observer.
function watchedOf(root) {
  for (let el = root; el; el = parentElementOf(el)) if (displayOf(el) !== 'contents') return el
  return bodyOf(document)
}

function register(seg) {
  seg.id = state.nextId++
  seg.applied = null
  seg.main = !!state.main && within(state.main, seg.root)
  state.segments.set(seg.id, seg)
  seg.watched = state.observer ? watchedOf(seg.root) : seg.root
  if (!state.byWatched.has(seg.watched)) state.byWatched.set(seg.watched, [])
  state.byWatched.get(seg.watched).push(seg)
  seg.group = among(state.groups, seg, () => ({ sig: seg.sig, segs: new Set(), sent: false, translation: null }))
  seg.group.segs.add(seg)
  for (const t of seg.texts) state.owner.set(t, seg)
  if (state.observer) state.observer.observe(seg.watched)
}

function add(segments) {
  for (const seg of segments) register(seg)
  for (const seg of segments) {
    const known = seg.group.translation || (among(kept.translations, seg) || {}).translation
    try {
      if (known) put(seg, known)
    } catch (err) {}
  }
  if (!state.observer) queue(segments)
}

function go(gen, source, target) {
  if (!state || state.gen !== gen || state.segments) return
  const pair = `${source}>${target}`
  if (kept.pair !== pair) kept = { pair, translations: new Map(), chars: 0 }
  state.main = mainContent()
  state.primary = primaryOf(source)
  state.spaced = !UNSPACED.has(primaryOf(target))
  state.segments = new Map()
  if (typeof IntersectionObserver === 'function') {
    state.observer = new IntersectionObserver(seen, { rootMargin: '400px 0px' })
  }
  add(collect(bodyOf(document)))
  watch()
}

function watch() {
  const body = bodyOf(document)
  if (typeof MutationObserver !== 'function' || !body) return
  state.mutations = new MutationObserver(changed)
  state.mutations.observe(body, { childList: true, subtree: true, characterData: true })
}

function quietly(fn) {
  try {
    fn()
  } finally {
    if (state && state.mutations) state.mutations.takeRecords()
  }
}

function changed(records) {
  if (!state) return
  for (const record of records) {
    try {
      const seg = record.type === 'characterData' ? state.owner.get(record.target) : null
      if (!seg) {
        mark(record.type === 'characterData' ? parentElementOf(record.target) : record.target)
        continue
      }
      mark(seg.root)
      if (churned(record.target)) state.given.add(record.target)
      quietly(() => drop(seg))
    } catch (err) {}
  }
  if (state.dirty.size && !state.rescan) {
    const s = state
    s.rescan = true
    setTimeout(() => rescan(s), s.delay)
  }
}

function mark(el) {
  if (el && connected(el)) state.dirty.add(el)
}

function churned(node) {
  const now = Date.now()
  const log = state.churn.get(node)
  if (!log || now - log.since > CHURN_WINDOW) {
    state.churn.set(node, { since: now, count: 1 })
    return false
  }
  log.count++
  return log.count > MAX_CHURN
}

// A segment the page changed or threw away.
function drop(seg) {
  if (seg.dead) return
  seg.dead = true
  if (seg.applied) restore(seg)
  for (const t of seg.texts) if (state.owner.get(t) === seg) state.owner.delete(t)
  state.segments.delete(seg.id)
  const group = seg.group
  group.segs.delete(seg)
  if (!group.segs.size && (!group.sent || group.translation)) unlist(group, seg.sent)
  const peers = (state.byWatched.get(seg.watched) || []).filter((each) => each !== seg)
  if (peers.length) state.byWatched.set(seg.watched, peers)
  else if (state.byWatched.delete(seg.watched) && state.observer) state.observer.unobserve(seg.watched)
}

function unlist(group, sent) {
  const rest = (state.groups.get(sent) || []).filter((entry) => entry !== group)
  if (rest.length) state.groups.set(sent, rest)
  else state.groups.delete(sent)
}

function rescan(s) {
  if (s !== state) return
  s.rescan = false
  const began = Date.now()
  quietly(() => {
    for (const seg of [...s.segments.values()]) {
      try {
        const intact =
          seg.texts.every((t) => connected(t)) &&
          seg.nodes.every((n) => parentOf(n) === seg.root) &&
          (seg.hidden || []).every((n) => kind(n) !== 1 || invisible(n))
        if (connected(seg.root) && intact) continue
        mark(seg.root)
        drop(seg)
      } catch (err) {}
    }
    const dirty = s.dirty
    s.dirty = new Set()
    for (const el of dirty) {
      let outer = connected(el)
      for (let p = parentElementOf(el); outer && p; p = parentElementOf(p)) if (dirty.has(p)) outer = false
      try {
        if (outer) add(collect(el))
      } catch (err) {}
    }
  })
  s.delay = Date.now() - began > SLOW_RESCAN ? Math.min(s.delay * 2, MAX_RESCAN_DELAY) : RESCAN_DELAY
}

function apply(gen, id, translation) {
  if (!state || state.gen !== gen || !state.segments) return
  const seg = state.segments.get(id) || state.inflight.get(id)
  state.inflight.delete(id)
  if (!seg || !usable(translation, seg)) return
  seg.group.translation = translation
  keep(seg, translation)
  if (!seg.group.segs.size) unlist(seg.group, seg.sent)
  quietly(() => {
    for (const each of seg.group.segs) {
      try {
        put(each, translation)
      } catch (err) {}
    }
  })
}

function keep(seg, translation) {
  const size = seg.sent.length + translation.reduce((n, r) => n + r.text.length, 0)
  if (among(kept.translations, seg) || kept.chars + size > MAX_KEPT) return
  among(kept.translations, seg, () => ({ sig: seg.sig, translation }))
  kept.chars += size
}

function revert() {
  if (!state) return
  if (state.mutations) state.mutations.disconnect()
  if (state.observer) state.observer.disconnect()
  for (const seg of state.segments ? state.segments.values() : []) {
    try {
      if (seg.applied) restore(seg)
    } catch (err) {}
  }
  state = null
}

window.addEventListener('pagehide', (event) => {
  if (event.persisted) revert()
})

window.__wisTranslate = { start, go, apply, revert }
