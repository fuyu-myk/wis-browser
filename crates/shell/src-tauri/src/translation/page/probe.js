// The page's language: what it declares and a sample of its text, read by
// `probe` after every load for the address bar's offer and by `start`. Single
// page apps change pages without loading one, so `watchMoves` asks for
// another probe once such a move has settled.

const MAX_LANG = 64
const SAMPLED = ['h1, h2, h3, p, blockquote', 'li, td, dd']
const MAIN = 'main, [role=main], article'
const UNSAMPLED = SKIP + ', nav, aside, header, footer, [role=navigation], [role=complementary]'
const MAX_SAMPLED = 1000
const MAX_SAMPLE = 1000
const MIN_SAMPLED = 8
const SETTLE = 1000

const whole = (key, value) => (typeof value === 'string' ? value.toWellFormed() : value)

function post(payload) {
  try {
    window.webkit.messageHandlers.wisTranslate.postMessage(JSON.stringify(payload, whole))
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

// The headings, paragraphs and lists of the main content or else the page,
// up to `room` characters of the text nodes `keep` takes. Menus are left out.
function sampled(room, keep) {
  const main = method(DocumentType, 'querySelector')(document, MAIN)
  const query = (selector) =>
    main
      ? method(ElementType, 'querySelectorAll')(main, selector)
      : method(DocumentType, 'querySelectorAll')(document, selector)
  const parts = []
  let size = 0
  for (const selector of SAMPLED) {
    if (size >= room) break
    for (const el of Array.from(query(selector)).slice(0, MAX_SAMPLED)) {
      if (size >= room) break
      if (closest(el, UNSAMPLED)) continue
      const text = textNodesIn(el)
        .filter(keep)
        .map((t) => t.nodeValue)
        .join('')
        .slice(0, room)
        .replace(/\s+/g, ' ')
        .trim()
      if (text.length < MIN_SAMPLED) continue
      parts.push(text)
      size += text.length + 1
    }
  }
  return parts
}

// The title, then the page's text, for telling the language it is in.
function sampleText() {
  const root = documentElementOf(document)
  const declared = primaryOf(declaredLanguage())
  const own = (t) => {
    const marked = closest(parentElementOf(t), '[lang]')
    return !marked || marked === root || (!!declared && primaryOf(attributeOf(marked, 'lang')) === declared)
  }
  const title = titleOf(document).replace(/\s+/g, ' ').trim()
  const room = MAX_SAMPLE - title.length
  let parts = sampled(room, own)
  if (!parts.length) parts = sampled(room, () => true)
  return [title, ...parts].filter(Boolean).join('\n').slice(0, MAX_SAMPLE)
}

function probe(id) {
  post({ kind: 'probe', id, lang: declaredLanguage(), sample: sampleText() })
}

// Where the page is. A fragment counts only as a hash router's route (one
// with a slash, or `#!`), not as an anchor.
function addressOf(href) {
  const at = href.indexOf('#')
  return at < 0 || /^#!|\//.test(href.slice(at)) ? href : href.slice(0, at)
}

function watchMoves() {
  if (typeof navigation === 'undefined') return
  let address = addressOf(location.href)
  let timer = null
  navigation.addEventListener('navigatesuccess', () => {
    clearTimeout(timer)
    timer = setTimeout(() => {
      const next = addressOf(location.href)
      if (next === address) return
      address = next
      post({ kind: 'moved' })
    }, SETTLE)
  })
}
