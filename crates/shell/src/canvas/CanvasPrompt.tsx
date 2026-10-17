import { useEffect, useLayoutEffect, useRef, useState, type DragEvent, type FormEvent, type KeyboardEvent } from 'react'


import { attachFiles, partialNote, partialTag, type Attachment } from '@/lib/canvasAttach'
import { dictated } from '@/lib/canvasDictation'
import { cx } from '@/lib/cx'

import { CanvasModelPicker } from './CanvasModelPicker'
import type { CanvasAgent } from './useCanvasAgent'

type Listening = 'off' | 'starting' | 'on'

const MAX_PROMPT_PX = 160
const NOTE_MS = 4000

function encode(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(String(reader.result).replace(/^data:[^,]*,/, ''))
    reader.onerror = () => reject(reader.error ?? new Error('The file could not be read.'))
    reader.readAsDataURL(file)
  })
}

export function CanvasPrompt({ agent }: { agent: CanvasAgent }) {
  const [text, setText] = useState('')
  const [files, setFiles] = useState<Attachment[]>([])
  const [note, setNote] = useState<string | null>(null)
  const [dropping, setDropping] = useState(false)
  const [multiline, setMultiline] = useState(false)
  const [pending, setPending] = useState<string[]>([])
  const [listening, setListening] = useState<Listening>('off')
  /** What was typed when dictation began; heard words follow it. */
  const typedBefore = useRef('')
  /** Whether heard words still drive the text; typing takes it back. */
  const following = useRef(false)
  const listeningRef = useRef<Listening>('off')
  listeningRef.current = listening
  const field = useRef<HTMLTextAreaElement>(null)
  const picker = useRef<HTMLInputElement>(null)
  const attached = useRef<Attachment[]>([])
  const queue = useRef<Promise<void>>(Promise.resolve())
  const keep = (next: Attachment[]) => {
    attached.current = next
    setFiles(next)
  }
  const busy = agent.activity !== 'idle'
  const available = agent.status?.available ?? false
  const documents = agent.status?.documents === true
  const reading = pending.length > 0

  useLayoutEffect(() => {
    const el = field.current
    if (!el) return
    el.style.height = 'auto'
    el.style.height = `${Math.min(el.scrollHeight, MAX_PROMPT_PX)}px`
    const style = getComputedStyle(el)
    const padding = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom)
    setMultiline(el.scrollHeight - padding > parseFloat(style.lineHeight) * 1.5)
  }, [text])

  const { mic } = agent
  useEffect(() => {
    // A dictation still running for this tab was left by a page that reloaded.
    void mic.stop().catch(() => null)
    const leave = mic.listen((event) => {
      if (event.type === 'heard') {
        if (following.current) setText(dictated(typedBefore.current, event.text))
      } else if (event.type === 'failed') {
        setNote(event.message)
      } else {
        following.current = false
        setListening('off')
      }
    })
    return () => {
      leave()
      if (listeningRef.current !== 'off') void mic.stop().catch(() => null)
    }
  }, [mic])

  const stopMic = (keep: boolean) => {
    following.current = keep
    void mic.stop().catch(() => null)
  }
  const toggleMic = async () => {
    if (listening !== 'off') {
      stopMic(true)
      return
    }
    typedBefore.current = text
    following.current = true
    setListening('starting')
    try {
      await mic.start()
      setListening((now) => (now === 'starting' ? 'on' : now))
    } catch (error) {
      following.current = false
      setListening('off')
      setNote(error instanceof Error ? error.message : 'Dictation could not start.')
    }
  }

  useEffect(() => {
    if (note === null) return
    const timer = window.setTimeout(() => setNote(null), NOTE_MS)
    return () => window.clearTimeout(timer)
  }, [note])

  const take = (list: readonly File[]) => {
    if (list.length === 0 || !available) return
    setPending((current) => [...current, ...list.map((f) => f.name)])
    queue.current = queue.current.then(async () => {
      try {
        const { added, notes } = await attachFiles(list, attached.current, {
          documents,
          readDocument: agent.readDocument,
          encode,
        })
        keep([...attached.current, ...added])
        setNote(notes.length > 0 ? notes.join(' · ') : null)
      } catch {
        setNote('The files could not be read.')
      } finally {
        setPending((current) => current.slice(list.length))
      }
      field.current?.focus()
    })
  }

  const submit = (e?: FormEvent) => {
    e?.preventDefault()
    if (busy) {
      if (e) agent.stop()
      return
    }
    if (text.trim() === '' || !available || reading) return
    if (listening !== 'off') stopMic(false)
    agent.send(text, files)
    setText('')
    keep([])
  }
  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault()
      submit()
    } else if (e.key === 'Escape') {
      if (listening !== 'off') stopMic(true)
      else e.currentTarget.blur()
    }
  }
  const carriesFiles = (e: DragEvent) => e.dataTransfer.types.includes('Files')

  return (
    <form
      className={cx('canvas-prompt', {
        'is-off': !available,
        'is-multiline': multiline || files.length > 0 || reading,
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
        aria-label="Attach files"
        title={documents ? 'Attach text, PDF, Word, RTF or ODT files' : 'Attach text files'}
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
        {(files.length > 0 || reading) && (
          <ul className="canvas-prompt-files" aria-label="Attached files">
            {files.map((file) => (
              <li key={file.name}>
                <span>{file.name}</span>
                {file.partial && (
                  <em className="canvas-prompt-partial" title={`Only ${partialNote(file.partial)} fit`}>
                    {partialTag(file.partial)}
                  </em>
                )}
                <button
                  type="button"
                  aria-label={`Remove ${file.name}`}
                  onClick={() => keep(attached.current.filter((f) => f.name !== file.name))}
                >
                  ×
                </button>
              </li>
            ))}
            {pending.map((name, i) => (
              <li key={`reading-${i}`} className="is-reading" aria-label={`Reading ${name}`}>
                <span>{name}</span>
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
          onChange={(e) => {
            if (listening !== 'off') stopMic(false)
            setText(e.currentTarget.value)
          }}
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
      {available && agent.status?.dictation === true && (
        <button
          type="button"
          className={cx('canvas-prompt-mic', { 'is-on': listening === 'on', 'is-starting': listening === 'starting' })}
          aria-label={listening === 'off' ? 'Tell the canvas agent something' : 'Stop listening'}
          aria-pressed={listening !== 'off'}
          title={listening === 'off' ? 'Tell the canvas agent something (on this Mac only)' : 'Stop listening'}
          onClick={() => void toggleMic()}
        >
          <svg viewBox="0 0 24 24" aria-hidden>
            <rect x="9" y="3" width="6" height="11" rx="3" />
            <path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21" />
          </svg>
        </button>
      )}
      <button
        type="submit"
        className={cx('canvas-prompt-send', { 'is-stop': busy })}
        disabled={!busy && (!available || reading || text.trim() === '')}
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
