// devtools.rs: REVEAL_JS, which finds the node page_menu.rs marked and inspects it.

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { rawStringConst } from '../support/rustSource.mjs'

const SCRIPT = rawStringConst('devtools.rs', 'REVEAL_JS').replace('__WIS_NONCE__', 'n1')

/** A root over `children`, searched one level deep as the script expects. */
function root(children) {
  return {
    querySelector: (selector) => children.find((c) => selector === `[data-wis-inspect="${c.attrs['data-wis-inspect']}"]`) || null,
    querySelectorAll: () => children,
  }
}

function element(attrs = {}, shadowChildren = null) {
  const e = {
    attrs: { ...attrs },
    removeAttribute: (name) => delete e.attrs[name],
    shadowRoot: shadowChildren ? root(shadowChildren) : null,
  }
  return e
}

function reveal(document) {
  const inspected = []
  new Function('document', 'inspect', SCRIPT)(document, (node) => inspected.push(node))
  return inspected
}

test('the node marked with this nonce is inspected and loses its mark', () => {
  const marked = element({ 'data-wis-inspect': 'n1' })
  assert.deepEqual(reveal(root([element(), marked])), [marked])
  assert.equal('data-wis-inspect' in marked.attrs, false)
})

test('a node marked inside nested open shadow roots is found', () => {
  const marked = element({ 'data-wis-inspect': 'n1' })
  const host = element({}, [element({}, [marked])])
  assert.deepEqual(reveal(root([element(), host])), [marked])
})

test('a mark the page planted with any other value is ignored', () => {
  assert.deepEqual(reveal(root([element({ 'data-wis-inspect': '' }), element({ 'data-wis-inspect': 'guess' })])), [])
})

test('with nothing marked nothing is inspected', () => {
  assert.deepEqual(reveal(root([element(), element({}, [element()])])), [])
})
