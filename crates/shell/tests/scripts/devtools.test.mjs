// devtools.rs: REVEAL_JS, which finds the node page_menu.rs marked and inspects it.

import assert from 'node:assert/strict'
import { test } from 'node:test'

import { rawStringConst } from '../support/rustSource.mjs'

const SCRIPT = rawStringConst('devtools.rs', 'REVEAL_JS')

/** A root over `children`, searched one level deep as the script expects. */
function root(children) {
  return {
    querySelector: () => children.find((c) => 'data-wis-inspect' in c.attrs) || null,
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

test('the marked node is inspected and loses its mark', () => {
  const marked = element({ 'data-wis-inspect': '' })
  assert.deepEqual(reveal(root([element(), marked])), [marked])
  assert.equal('data-wis-inspect' in marked.attrs, false)
})

test('a node marked inside nested open shadow roots is found', () => {
  const marked = element({ 'data-wis-inspect': '' })
  const host = element({}, [element({}, [marked])])
  assert.deepEqual(reveal(root([element(), host])), [marked])
})

test('with nothing marked nothing is inspected', () => {
  assert.deepEqual(reveal(root([element(), element({}, [element()])])), [])
})
