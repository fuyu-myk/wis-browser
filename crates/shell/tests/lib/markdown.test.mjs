// The assistant's markdown subset: inline spans and block structure (`lib/markdown.ts`).

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { parseMarkdown, parseSpans } from '../../src/lib/markdown.ts'

const text = (t) => ({ kind: 'text', text: t })
const paragraph = (...spans) => ({ kind: 'paragraph', spans })

describe('inline spans', () => {
  test('plain text is a single text span', () => {
    assert.deepEqual(parseSpans('just words'), [text('just words')])
  })

  test('empty text has no spans', () => {
    assert.deepEqual(parseSpans(''), [])
  })

  test('a code span splits the text around it', () => {
    assert.deepEqual(parseSpans('run `cargo test` now'), [
      text('run '),
      { kind: 'code', text: 'cargo test' },
      text(' now'),
    ])
  })

  test('a double-backtick code span may hold a backtick, and is trimmed', () => {
    assert.deepEqual(parseSpans('`` a`b ``'), [{ kind: 'code', text: 'a`b' }])
  })

  test('nothing inside a code span is styled', () => {
    assert.deepEqual(parseSpans('`**x** [a](https://b.test)`'), [{ kind: 'code', text: '**x** [a](https://b.test)' }])
  })

  test('an unclosed backtick stays literal', () => {
    assert.deepEqual(parseSpans('a `b'), [text('a `b')])
  })

  test('a link carries its text and href', () => {
    assert.deepEqual(parseSpans('see [the docs](https://x.test/a?b=1) here'), [
      text('see '),
      { kind: 'link', text: 'the docs', href: 'https://x.test/a?b=1' },
      text(' here'),
    ])
  })

  test('a link href is kept verbatim for the renderer to vet', () => {
    assert.deepEqual(parseSpans('[x](javascript:alert(1))'), [
      { kind: 'link', text: 'x', href: 'javascript:alert(1' },
      text(')'),
    ])
  })

  test('emphasis inside link text stays literal', () => {
    assert.deepEqual(parseSpans('[**x**](https://u.test)'), [{ kind: 'link', text: '**x**', href: 'https://u.test' }])
  })

  test('a link target with a space is not a link', () => {
    assert.deepEqual(parseSpans('[a](b c)'), [text('[a](b c)')])
  })

  test('strong, emphasis and strike each take both of their markers', () => {
    assert.deepEqual(parseSpans('**a** __b__ *c* _d_ ~~e~~'), [
      { kind: 'strong', text: 'a' },
      text(' '),
      { kind: 'strong', text: 'b' },
      text(' '),
      { kind: 'em', text: 'c' },
      text(' '),
      { kind: 'em', text: 'd' },
      text(' '),
      { kind: 'strike', text: 'e' },
    ])
  })

  test('snake_case identifiers stay literal', () => {
    assert.deepEqual(parseSpans('call read_file_name and a_b_c'), [text('call read_file_name and a_b_c')])
  })

  test('emphasis does not run across a line break', () => {
    assert.deepEqual(parseSpans('*a\nb*'), [text('*a\nb*')])
  })

  test('spaced asterisks in arithmetic stay literal', () => {
    assert.deepEqual(parseSpans('2 * 3 * 4 = 24'), [text('2 * 3 * 4 = 24')])
    assert.deepEqual(parseSpans('2 ** 3 ** 4'), [text('2 ** 3 ** 4')])
  })

  test('emphasis hugging its words still applies inside a sentence', () => {
    assert.deepEqual(parseSpans('a *big* deal'), [text('a '), { kind: 'em', text: 'big' }, text(' deal')])
    assert.deepEqual(parseSpans('**x**'), [{ kind: 'strong', text: 'x' }])
  })
})

describe('blocks', () => {
  test('empty text has no blocks', () => {
    assert.deepEqual(parseMarkdown(''), [])
    assert.deepEqual(parseMarkdown('\n  \n'), [])
  })

  test('blank lines separate paragraphs; lines within one are kept together', () => {
    assert.deepEqual(parseMarkdown('a\nb\n\nc'), [paragraph(text('a\nb')), paragraph(text('c'))])
  })

  test('headings deeper than three render at level three', () => {
    assert.deepEqual(
      parseMarkdown('# One\n## Two\n### Three\n###### Six').map((b) => b.level),
      [1, 2, 3, 3],
    )
  })

  test('heading text is styled', () => {
    assert.deepEqual(parseMarkdown('## **Bold** title'), [
      { kind: 'heading', level: 2, spans: [{ kind: 'strong', text: 'Bold' }, text(' title')] },
    ])
  })

  test('a hash without a following space is not a heading', () => {
    assert.deepEqual(parseMarkdown('#hashtag'), [paragraph(text('#hashtag'))])
  })

  test('a fenced block keeps its language and its text unstyled, blank lines included', () => {
    assert.deepEqual(parseMarkdown('```ts\nconst a = 1\n\n**x**\n```'), [
      { kind: 'code', lang: 'ts', text: 'const a = 1\n\n**x**' },
    ])
  })

  test('a fence without a language has an empty one', () => {
    assert.deepEqual(parseMarkdown('```\nx\n```'), [{ kind: 'code', lang: '', text: 'x' }])
  })

  test('a fence still streaming in runs to the end of the text', () => {
    assert.deepEqual(parseMarkdown('```rust\nfn main() {\n    let'), [
      { kind: 'code', lang: 'rust', text: 'fn main() {\n    let' },
    ])
  })

  test('a fence ends the paragraph before it and the text after starts a new one', () => {
    assert.deepEqual(parseMarkdown('intro\n```\nx\n```\nafter'), [
      paragraph(text('intro')),
      { kind: 'code', lang: '', text: 'x' },
      paragraph(text('after')),
    ])
  })

  test('three or more dashes, stars or underscores are a rule', () => {
    for (const line of ['---', '***', '___', '   -----', '--- ']) {
      assert.deepEqual(parseMarkdown(line), [{ kind: 'rule' }], line)
    }
  })

  test('a rule under a line ends the paragraph', () => {
    assert.deepEqual(parseMarkdown('Title\n---'), [paragraph(text('Title')), { kind: 'rule' }])
  })

  test('consecutive quote lines form one quote, with or without a space after >', () => {
    assert.deepEqual(parseMarkdown('> a\n>b\n>\n> **c**\nafter'), [
      { kind: 'quote', lines: [[text('a')], [text('b')], [], [{ kind: 'strong', text: 'c' }]] },
      paragraph(text('after')),
    ])
  })

  test('bullet items with any marker form one unordered list', () => {
    assert.deepEqual(parseMarkdown('- a\n* b\n+ *c*'), [
      {
        kind: 'list',
        ordered: false,
        items: [
          { depth: 0, spans: [text('a')] },
          { depth: 0, spans: [text('b')] },
          { depth: 0, spans: [{ kind: 'em', text: 'c' }] },
        ],
      },
    ])
  })

  test('numbered items with a dot or parenthesis form an ordered list', () => {
    const [list] = parseMarkdown('1. a\n2) b\n10. c')
    assert.equal(list.ordered, true)
    assert.equal(list.items.length, 3)
  })

  test('indentation nests by two spaces, at most two levels deep', () => {
    const [list] = parseMarkdown('- a\n  - b\n    - c\n        - d')
    assert.deepEqual(
      list.items.map((i) => i.depth),
      [0, 1, 2, 2],
    )
  })

  test('a list ends at the first line that is not an item', () => {
    assert.deepEqual(parseMarkdown('lead\n- a\nafter').map((b) => b.kind), ['paragraph', 'list', 'paragraph'])
  })

  test('a year followed by a full stop is not a list item', () => {
    assert.deepEqual(parseMarkdown('2024. A good year'), [paragraph(text('2024. A good year'))])
  })

  test('a pipe row over a separator is a table, with styled and escaped cells', () => {
    const md = '| Name | Value |\n| :--- | ---: |\n| a | **b** |\n| c \\| d | e |\nafter'
    assert.deepEqual(parseMarkdown(md), [
      {
        kind: 'table',
        header: [[text('Name')], [text('Value')]],
        rows: [
          [[text('a')], [{ kind: 'strong', text: 'b' }]],
          [[text('c | d')], [text('e')]],
        ],
      },
      paragraph(text('after')),
    ])
  })

  test('a separator without outer pipes still makes a table', () => {
    const [table] = parseMarkdown('intro\n| a | b |\n--- | ---\n| 1 | 2 |').slice(1)
    assert.equal(table.kind, 'table')
    assert.equal(table.rows.length, 1)
  })

  test('a table header with no body has no rows', () => {
    assert.deepEqual(parseMarkdown('| a | b |\n|---|---|'), [
      { kind: 'table', header: [[text('a')], [text('b')]], rows: [] },
    ])
  })

  test('pipe rows without a separator stay a paragraph', () => {
    assert.deepEqual(parseMarkdown('| a | b |\n| c | d |'), [paragraph(text('| a | b |\n| c | d |'))])
  })

  test('a single-column table is a table', () => {
    assert.deepEqual(parseMarkdown('| a |\n|---|\n| 1 |'), [
      { kind: 'table', header: [[text('a')]], rows: [[[text('1')]]] },
    ])
  })

  test('every prefix of a streamed answer parses', () => {
    const answer =
      '# Plan\n\nSome **bold** and `code`.\n\n```js\nlet x = 1\n```\n\n- one\n  - two\n\n> quoted\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n---\n[link](https://x.test)'
    for (let end = 0; end <= answer.length; end += 1) {
      assert.ok(Array.isArray(parseMarkdown(answer.slice(0, end))), `prefix of length ${end}`)
    }
    assert.deepEqual(
      parseMarkdown(answer).map((b) => b.kind),
      ['heading', 'paragraph', 'code', 'list', 'quote', 'table', 'rule', 'paragraph'],
    )
  })
})
