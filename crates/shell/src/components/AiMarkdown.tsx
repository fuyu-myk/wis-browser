import { createContext, Fragment, memo, useContext, type ReactNode } from 'react'

import { parseMarkdown, type Block, type ListItem, type Span } from '@/lib/markdown'
import { hostOf } from '@/lib/ai'
import { splitCitations } from '@/lib/research'
import { isWebUrl } from '@/lib/url'

/** A research answer's citations: `[n]` up to `count` becomes a button. */
interface Citations {
  count: number
  onCite: (n: number) => void
}

const CitationsContext = createContext<Citations | undefined>(undefined)

interface AiMarkdownProps {
  text: string
  /** Receives only http(s) hrefs; refused schemes render as plain text. The
   *  visible text rides along for the deceptive-text check. */
  onOpenLink: (href: string, text: string) => void
  citations?: Citations
}

function PlainText({ text }: { text: string }) {
  const citations = useContext(CitationsContext)
  if (citations === undefined) return <>{text}</>
  return (
    <>
      {splitCitations(text, citations.count).map((part, i) =>
        typeof part === 'number' ? (
          <button
            key={i}
            type="button"
            className="kb-ai-cite"
            aria-label={`Source ${part}`}
            onClick={() => citations.onCite(part)}
          >
            {part}
          </button>
        ) : (
          <Fragment key={i}>{part}</Fragment>
        ),
      )}
    </>
  )
}

function SpanView({
  span,
  onOpenLink,
}: {
  span: Span
  onOpenLink: (href: string, text: string) => void
}) {
  switch (span.kind) {
    case 'strong':
      return <strong>{span.text}</strong>
    case 'em':
      return <em>{span.text}</em>
    case 'strike':
      return <s>{span.text}</s>
    case 'code':
      return <code className="kb-ai-md-code">{span.text}</code>
    case 'link': {
      if (!isWebUrl(span.href)) return <>{span.text}</>
      return (
        <button
          type="button"
          className="kb-ai-md-link"
          title={hostOf(span.href)}
          onClick={() => onOpenLink(span.href, span.text)}
        >
          {span.text}
        </button>
      )
    }
    default:
      return <PlainText text={span.text} />
  }
}

function spansOf(spans: Span[], onOpenLink: (href: string, text: string) => void): ReactNode {
  return spans.map((span, i) => <SpanView key={i} span={span} onOpenLink={onOpenLink} />)
}

/** Group a flat depth-tagged item run into nested lists. */
function listOf(
  items: ListItem[],
  ordered: boolean,
  depth: number,
  onOpenLink: (href: string, text: string) => void,
): ReactNode {
  const out: ReactNode[] = []
  let i = 0
  while (i < items.length) {
    if (items[i].depth < depth) break
    if (items[i].depth === depth) {
      const children: ListItem[] = []
      let j = i + 1
      while (j < items.length && items[j].depth > depth) {
        children.push(items[j])
        j += 1
      }
      out.push(
        <li key={i}>
          {spansOf(items[i].spans, onOpenLink)}
          {children.length > 0 && listOf(children, ordered, depth + 1, onOpenLink)}
        </li>,
      )
      i = j
    } else {
      i += 1
    }
  }

  const Tag = ordered ? 'ol' : 'ul'
  return <Tag>{out}</Tag>
}

function BlockView({
  block,
  onOpenLink,
}: {
  block: Block
  onOpenLink: (href: string, text: string) => void
}) {
  switch (block.kind) {
    case 'paragraph':
      return <p>{spansOf(block.spans, onOpenLink)}</p>
    case 'heading': {
      const Tag = (['h3', 'h4', 'h5'] as const)[block.level - 1]
      return <Tag>{spansOf(block.spans, onOpenLink)}</Tag>
    }
    case 'code':
      return (
        <pre className="kb-ai-md-pre" data-lang={block.lang || undefined}>
          <code>{block.text}</code>
        </pre>
      )
    case 'list':
      return <>{listOf(block.items, block.ordered, 0, onOpenLink)}</>
    case 'quote':
      return (
        <blockquote>
          {block.lines.map((line, i) => (
            <Fragment key={i}>
              {i > 0 && <br />}
              {spansOf(line, onOpenLink)}
            </Fragment>
          ))}
        </blockquote>
      )
    case 'table':
      return (
        <div className="kb-ai-md-tablewrap">
          <table>
            <thead>
              <tr>
                {block.header.map((cell, i) => (
                  <th key={i}>{spansOf(cell, onOpenLink)}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              {block.rows.map((row, i) => (
                <tr key={i}>
                  {row.map((cell, j) => (
                    <td key={j}>{spansOf(cell, onOpenLink)}</td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )
    case 'rule':
      return <hr />
    default:
      return null
  }
}

/**
 * The assistant answer as markdown. Every node is an escaped React element
 * from the typed tree in `lib/markdown`. Memoised because the transcript
 * re-renders per streamed frame and only the last turn's text actually changes.
 */
export const AiMarkdown = memo(function AiMarkdown({
  text,
  onOpenLink,
  citations,
}: AiMarkdownProps) {
  return (
    <CitationsContext.Provider value={citations}>
      <div className="kb-ai-text kb-ai-md">
        {parseMarkdown(text).map((block, i) => (
          <BlockView key={i} block={block} onOpenLink={onOpenLink} />
        ))}
      </div>
    </CitationsContext.Provider>
  )
})
