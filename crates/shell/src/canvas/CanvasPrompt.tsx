import { useEffect, useLayoutEffect, useRef, useState, type DragEvent, type FormEvent, type KeyboardEvent } from 'react'


import { acceptFile, MAX_FILE_CHARS, type Attachment } from '@/lib/canvasAttach'
import { cx } from '@/lib/cx'

import { CanvasModelPicker } from './CanvasModelPicker'
import type { CanvasAgent } from './useCanvasAgent'

const MAX_PROMPT_PX = 160
const NOTE_MS = 4000
/** UTF-8 spends at most four bytes a character. */
const MAX_FILE_BYTES = MAX_FILE_CHARS * 4

async function attach(files: readonly File[], taken: readonly Attachment[]) {
  const kept = [...taken]
  const refused: string[] = []
  for (const file of files) {
    const text = file.size > MAX_FILE_BYTES ? 'x'.repeat(MAX_FILE_CHARS + 1) : await file.text().catch(() => '\u0000')
    const result = acceptFile(file.name, text, kept)
    if (result.ok) kept.push(result.file)
    else refused.push(result.reason)
  }
  return { kept, refused }
}

export function CanvasPrompt({ agent }: { agent: CanvasAgent }) {
  const [text, setText] = useState('')
  const [files, setFiles] = useState<Attachment[]>([])
  const [note, setNote] = useState<string | null>(null)
  const [dropping, setDropping] = useState(false)
  const [multiline, setMultiline] = useState(false)
  const field = useRef<HTMLTextAreaElement>(null)
  const picker = useRef<HTMLInputElement>(null)
  const attached = useRef<Attachment[]>([])
  const reading = useRef<Promise<void>>(Promise.resolve())
  const keep = (next: Attachment[]) => {
    attached.current = next
    setFiles(next)
  }
  const busy = agent.activity !== 'idle'
  const available = agent.status?.available ?? false

  useLayoutEffect(() => {
    const el = field.current
    if (!el) return
    el.style.height = 'auto'
    el.style.height = `${Math.min(el.scrollHeight, MAX_PROMPT_PX)}px`
    const style = getComputedStyle(el)
    const padding = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom)
    setMultiline(el.scrollHeight - padding > parseFloat(style.lineHeight) * 1.5)
  }, [text])

  useEffect(() => {
    if (note === null) return
    const timer = window.setTimeout(() => setNote(null), NOTE_MS)
    return () => window.clearTimeout(timer)
  }, [note])

  const take = (list: readonly File[]) => {
    if (list.length === 0 || !available) return
    reading.current = reading.current.then(async () => {
      const { kept, refused } = await attach(list, attached.current)
      keep(kept)
      setNote(refused.length > 0 ? refused.join(' · ') : null)
      field.current?.focus()
    })
  }

  const submit = (e?: FormEvent) => {
    e?.preventDefault()
    if (busy) {
      if (e) agent.stop()
      return
    }
    if (text.trim() === '' || !available) return
    agent.send(text, files)
    setText('')
    keep([])
  }
  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault()
      submit()
    } else if (e.key === 'Escape') {
      e.currentTarget.blur()
    }
  }
  const carriesFiles = (e: DragEvent) => e.dataTransfer.types.includes('Files')

  return (
    <form
      className={cx('canvas-prompt', {
        'is-off': !available,
        'is-multiline': multiline || files.length > 0,
        'is-dropping': dropping,
      })}
      onSubmit={submit}
      onDragOver={(e) => {
        if (!carriesFiles(e) || !available) return
        e.preventDefault()
        setDropping(true)
      }}
      onDragLeave={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDropping(false)
      }}
      onDrop={(e) => {
        if (!carriesFiles(e)) return
        e.preventDefault()
        setDropping(false)
        take([...e.dataTransfer.files])
      }}
    >
      {note !== null && <p className="canvas-prompt-note">{note}</p>}
      <button
        type="button"
        className="canvas-prompt-attach"
        disabled={!available}
        aria-label="Attach text files"
        title="Attach text files"
        onClick={() => picker.current?.click()}
      >
        <svg viewBox="0 0 24 24" aria-hidden>
          <path d="m21 11-8.6 8.6a5 5 0 0 1-7.1-7.1l8.6-8.6a3.3 3.3 0 0 1 4.7 4.7l-8.6 8.6a1.7 1.7 0 0 1-2.4-2.4l7.9-7.9" />
        </svg>
      </button>
      <input
        ref={picker}
        type="file"
        multiple
        hidden
        onChange={(e) => {
          take([...(e.currentTarget.files ?? [])])
          e.currentTarget.value = ''
        }}
      />
      <div className="canvas-prompt-body">
        {files.length > 0 && (
          <ul className="canvas-prompt-files" aria-label="Attached files">
            {files.map((file) => (
              <li key={file.name}>
                <span>{file.name}</span>
                <button
                  type="button"
                  aria-label={`Remove ${file.name}`}
                  onClick={() => keep(attached.current.filter((f) => f.name !== file.name))}
                >
                  ×
                </button>
              </li>
            ))}
          </ul>
        )}
        <textarea
          ref={field}
          rows={1}
          value={text}
          disabled={!available}
          aria-label="Ask the canvas agent"
          placeholder={
            agent.status === null || available
              ? 'Ask the canvas agent'
              : 'Set up AI in Settings › AI to use the canvas agent'
          }
          onChange={(e) => setText(e.currentTarget.value)}
          onFocus={agent.refresh}
          onKeyDown={onKeyDown}
        />
      </div>
      {available && agent.status?.model && (
        <CanvasModelPicker
          model={agent.status.model}
          load={agent.listModels}
          choose={agent.setModel}
          onError={setNote}
        />
      )}
      <button
        type="submit"
        className={cx('canvas-prompt-send', { 'is-stop': busy })}
        disabled={!busy && (!available || text.trim() === '')}
        aria-label={busy ? 'Stop' : 'Send'}
        title={busy ? 'Stop' : 'Send'}
      >
        <svg viewBox="0 0 24 24" aria-hidden>
          {busy ? <rect x="7" y="7" width="10" height="10" rx="2" /> : <path d="M12 19V5M5 12l7-7 7 7" />}
        </svg>
      </button>
    </form>
  )
}
