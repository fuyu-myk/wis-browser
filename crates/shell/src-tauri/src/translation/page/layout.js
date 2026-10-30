// A translation laid out in place of a segment's text, and the page put
// back as it was.
const LEADING = /^([\p{Pe}\p{Pf}.,;:!?%…、。]+)(\s*)/u

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

function reads(text, words) {
  const escaped = words.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  return new RegExp(`(?<![\\p{L}\\p{M}\\p{N}])${escaped}(?![\\p{L}\\p{M}\\p{N}])`, 'u').test(text)
}

function usable(runs, seg) {
  if (!textRuns(runs)) return false
  const text = runs.map((r) => r.text).join('')
  if (blank(text) && !blank(seg.sent)) return false
  const tags = new Set()
  for (const r of runs) {
    if (r.tag == null) continue
    if (!Number.isInteger(r.tag) || r.tag < 0 || r.tag >= seg.tagged.length) return false
    tags.add(r.tag)
  }
  const kept = (tag) =>
    seg.long && seg.opaque.has(tag) && !seg.objects.includes(tag) &&
    reads(text, seg.runs.find((r) => r.tag === tag).text.trim())
  const back = (tag) => tags.has(tag) || kept(tag)
  return seg.required.every(back) && (seg.long || seg.needed.every((tag) => tags.has(tag)))
}

function settled(runs, seg) {
  const loose = (run) => run.tag == null || !seg.needed.includes(run.tag)
  const seen = new Set()
  const out = []
  const plain = (text) => {
    const prev = out[out.length - 1]
    if (prev && prev.tag == null) out[out.length - 1] = { ...prev, text: prev.text + text }
    else out.push({ text })
  }
  for (const original of runs) {
    const again = original.tag != null && loose(original) && seen.has(original.tag)
    let run = again ? { text: original.text } : original
    if (original.tag != null) seen.add(original.tag)
    const prev = out[out.length - 1]
    const lead = prev && loose(prev) && loose(run) ? LEADING.exec(run.text) : null
    if (lead) {
      out[out.length - 1] = { ...prev, text: prev.text + lead[1] }
      if (lead[2]) plain(lead[2])
      run = { ...run, text: run.text.slice(lead[0].length) }
      if (!run.text) continue
    }
    if (run.tag == null) plain(run.text)
    else out.push(run)
  }
  return out
}

const shownInline = (el) => /^inline/.test(displayOf(el))

// The last character shown in `node`, '' after a line break or an image,
// or undefined when it shows nothing.
function lastShown(node) {
  const type = kind(node)
  if (type === 3) return node.nodeValue ? node.nodeValue.slice(-1) : undefined
  if (type !== 1) return undefined
  const tag = tagOf(node)
  if (BREAKS.has(tag) || (BLOCKS.has(tag) && !shownInline(node))) return ''
  if (matches(node, SKIP) && !opaque(node)) return undefined
  const kids = childrenOf(node)
  for (let i = kids.length - 1; i >= 0; i--) {
    const shown = lastShown(kids[i])
    if (shown !== undefined) return shown
  }
  return undefined
}

// The last character shown before `node` in its block, or '' after a line
// break, an image or the start of the block.
function textBefore(node) {
  for (let n = node; n; n = parentOf(n)) {
    for (let prev = previousOf(n); prev; prev = previousOf(prev)) {
      const shown = lastShown(prev)
      if (shown !== undefined) return shown
    }
    const parent = parentOf(n)
    if (!parent || kind(parent) !== 1 || (BLOCKS.has(tagOf(parent)) && !shownInline(parent))) return ''
  }
  return ''
}

function gapBefore(seg, runs) {
  const text = runs.map((r) => r.text).join('')
  if (!state.spaced || !text || /^[\s\p{Pe}\p{Pf}.,;:!?%…、。]/u.test(text)) return false
  const last = textBefore(seg.whole || seg.nodes[0])
  return !!last && !/[\s\p{Ps}\p{Pi}¿¡$]/u.test(last)
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

// Lays `nodes` out in order where the segment's nodes stood, moving text
// rather than elements and as little as it can.
function arrange(root, nodes) {
  const kids = childrenOf(root)
  const at = new Map(kids.map((n, i) => [n, i]))
  const positions = nodes.map((n) => (at.has(n) ? at.get(n) : -1))
  const keep = staying(positions, nodes.map((n) => (kind(n) === 1 ? nodes.length + 1 : 1)))
  let ref = kids[Math.max(...positions) + 1] || null
  for (let i = nodes.length - 1; i >= 0; i--) {
    if (!keep.has(i)) insertBefore(root, nodes[i], ref)
    ref = nodes[i]
  }
}

// The heaviest set of nodes already in order, by index; -1 is a new node.
function staying(positions, weights) {
  const best = []
  const prev = []
  let end = -1
  positions.forEach((p, i) => {
    best[i] = -1
    prev[i] = -1
    if (p < 0) return
    best[i] = weights[i]
    for (let j = 0; j < i; j++) {
      if (best[j] >= 0 && positions[j] < p && best[j] + weights[i] > best[i]) {
        best[i] = best[j] + weights[i]
        prev[i] = j
      }
    }
    if (end < 0 || best[i] > best[end]) end = i
  })
  const keep = new Set()
  for (let i = end; i >= 0; i = prev[i]) keep.add(i)
  return keep
}

const inOrder = (a, b) => a.length === b.length && a.every((n, i) => n === b[i])

function put(seg, translation) {
  if (seg.applied || seg.dead || !usable(translation, seg) || currentText(seg) !== seg.sent) return false
  if (!seg.whole && seg.nodes.some((n) => parentOf(n) !== seg.root)) return false
  let runs = withEdges(settled(translation, seg), seg.sent)
  const gap = gapBefore(seg, runs)
  const saved = new Map()
  const created = []
  const applied = { created, saved, written: null }
  seg.applied = applied
  if (seg.whole) {
    setText(seg.whole, runs.map((r) => r.text).join(''), saved)
    if (gap) {
      const space = createText(' ')
      insertBefore(seg.root, space, seg.whole)
      created.push(space)
    }
    applied.written = writtenIn(saved)
    return true
  }
  if (gap && runs[0].tag == null) runs = [{ ...runs[0], text: ' ' + runs[0].text }, ...runs.slice(1)]
  else if (gap) runs = [{ text: ' ', edge: true }, ...runs]
  const loose = seg.nodes.filter((n) => kind(n) === 3)
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
  seg.tagged.forEach((el, tag) => {
    if (used.has(tag)) return
    setText(el, '', saved)
    laid.push(el)
  })
  for (; next < loose.length; next++) {
    remember(saved, loose[next])
    loose[next].nodeValue = ''
    laid.push(loose[next])
  }
  for (const n of created) for (const t of textsIn(n)) state.owner.set(t, seg)
  applied.written = writtenIn(saved)
  const order = withSilent(seg, laid)
  if (!inOrder(order, seg.nodes)) arrange(seg.root, order)
  return true
}

function withSilent(seg, laid) {
  const out = laid.slice()
  let after = null
  for (const n of seg.nodes) {
    if (textual(n)) {
      after = n
      continue
    }
    out.splice(after ? out.indexOf(after) + 1 : 0, 0, n)
    after = n
  }
  return out
}

const writtenIn = (saved) => new Map([...saved.keys()].map((t) => [t, t.nodeValue]))
const textsIn = (n) => (kind(n) === 3 ? [n] : textNodesIn(n))

function restore(seg) {
  const { created, saved, written } = seg.applied
  for (const n of created) {
    for (const t of textsIn(n)) if (state.owner.get(t) === seg) state.owner.delete(t)
    if (parentOf(n) === seg.root) removeChild(seg.root, n)
  }
  if (!seg.whole && seg.nodes.every((n) => parentOf(n) === seg.root)) {
    const members = new Set(seg.nodes)
    const now = childrenOf(seg.root).filter((n) => members.has(n))
    if (!inOrder(now, seg.nodes)) arrange(seg.root, seg.nodes)
  }
  for (const [t, value] of saved) if (!written || t.nodeValue === written.get(t)) t.nodeValue = value
  seg.applied = null
}
