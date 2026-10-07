// A fake tab page for `video_popout/script.rs`: an element tree with videos, a virtual clock, and a wisPopout recorder.

import { rawStringConst } from './rustSource.mjs'
import { createClock } from './s3-clock.mjs'
import { evalIn, rustLiteral } from './s3-native.mjs'

const SCRIPT = rawStringConst('video_popout/script.rs', 'POPOUT_JS')

/** What the native side evaluates in the page, as it ships. */
export const NATIVE = {
  enter: rustLiteral('video_popout/mod.rs', '__wisPopoutEnter('),
  exit: rustLiteral('video_popout/mod.rs', '__wisPopoutExit('),
  resume: rustLiteral('video_popout/mod.rs', '__wisPopoutResume('),
  enterPip: rustLiteral('video_popout/mod.rs', '__wisEnterPip('),
  exitPip: rustLiteral('video_popout/mod.rs', '__wisExitPip('),
  hoverOn: rustLiteral('video_popout/imp.rs', '__wisPopoutHover(true)'),
  hoverOff: rustLiteral('video_popout/imp.rs', '__wisPopoutHover(false)'),
  pipFlag: rustLiteral('video_popout/script.rs', '__wisPopoutPip = true'),
}

function makeEvent(type, init) {
  return {
    type,
    isTrusted: true,
    defaultPrevented: false,
    propagationStopped: false,
    preventDefault() {
      this.defaultPrevented = true
    },
    stopPropagation() {
      this.propagationStopped = true
    },
    ...init,
  }
}

function detach(node) {
  const parent = node.parentNode
  if (!parent) return
  parent.children.splice(parent.children.indexOf(node), 1)
  node.parentNode = null
}

function walk(node, out = []) {
  out.push(node)
  node.children.forEach((child) => walk(child, out))
  return out
}

function makeNode(page, tag) {
  const classes = new Set()
  const attrs = new Map()
  const listeners = new Map()
  const node = {
    nodeType: 1,
    tagName: tag.toUpperCase(),
    id: '',
    parentNode: null,
    children: [],
    style: {},
    rect: null,
    clientWidth: page.viewportWidth,
    classList: {
      add: (c) => void classes.add(c),
      remove: (c) => void classes.delete(c),
      contains: (c) => classes.has(c),
      toggle(c, force) {
        const on = force ?? !classes.has(c)
        if (on) classes.add(c)
        else classes.delete(c)
        return on
      },
    },
    get className() {
      return [...classes].join(' ')
    },
    set className(value) {
      classes.clear()
      for (const c of String(value).split(/\s+/)) if (c) classes.add(c)
    },
    get isConnected() {
      let n = node
      while (n.parentNode) n = n.parentNode
      return n === page.root
    },
    get nextSibling() {
      const siblings = node.parentNode?.children
      return siblings?.[siblings.indexOf(node) + 1] ?? null
    },
    setAttribute: (name, value) => void attrs.set(name, String(value)),
    getAttribute: (name) => attrs.get(name) ?? null,
    appendChild(child) {
      detach(child)
      child.parentNode = node
      node.children.push(child)
      return child
    },
    insertBefore(child, ref) {
      if (!ref) return node.appendChild(child)
      detach(child)
      const at = node.children.indexOf(ref)
      if (at < 0) throw new Error('NotFoundError')
      child.parentNode = node
      node.children.splice(at, 0, child)
      return child
    },
    remove: () => detach(node),
    contains(other) {
      for (let n = other; n; n = n.parentNode) if (n === node) return true
      return false
    },
    addEventListener(type, fn, options) {
      if (!listeners.has(type)) listeners.set(type, [])
      listeners.get(type).push({ fn, options })
    },
    removeEventListener(type, fn) {
      const list = listeners.get(type)
      if (list) listeners.set(type, list.filter((l) => l.fn !== fn))
    },
    listenerCount: (type) => (listeners.get(type) || []).length,
    fire(type, init = {}) {
      const event = makeEvent(type, { target: node, ...init })
      for (const { fn } of [...(listeners.get(type) || [])]) fn.call(node, event)
      return event
    },
    getBoundingClientRect() {
      const { left = 0, top = 0, width = 0, height = 0 } = node.rect || {}
      return { left, top, width, height, right: left + width, bottom: top + height }
    },
    setPointerCapture(id) {
      node.capturedPointer = id
    },
  }
  return node
}

function makeVideo(page, spec) {
  const {
    playing = false,
    muted = false,
    ended = false,
    left = 0,
    top = 0,
    width = 640,
    height = 360,
    videoWidth = 1920,
    videoHeight = 1080,
    duration = 600,
    currentTime = 0,
    volume = 1,
  } = spec
  const v = makeNode(page, 'video')
  let time = currentTime
  Object.assign(v, {
    paused: !playing,
    ended,
    muted,
    volume,
    duration,
    videoWidth,
    videoHeight,
    seeking: false,
    error: null,
    disablePictureInPicture: false,
    refuseSeek: false,
    rect: { left, top, width, height },
    plays: 0,
    pauses: 0,
    seeks: [],
    fastSeeks: [],
    pipRequests: 0,
  })
  Object.defineProperty(v, 'currentTime', {
    get: () => time,
    set(t) {
      if (v.refuseSeek) throw new Error('InvalidStateError')
      time = t
      v.seeks.push(t)
    },
  })
  v.play = () => {
    v.plays++
    v.paused = false
    return page.playRejects ? Promise.reject(new Error('NotAllowedError')) : Promise.resolve()
  }
  v.pause = () => {
    v.pauses++
    v.paused = true
  }
  v.fastSeek = (t) => {
    time = t
    v.fastSeeks.push(t)
  }
  v.requestPictureInPicture = () => {
    v.pipRequests++
    if (page.pipThrows) throw new Error('NotAllowedError')
    return page.pipRejects ? Promise.reject(new Error('NotAllowedError')) : Promise.resolve()
  }
  return v
}

function registry() {
  const map = new Map()
  return {
    add(type, fn, options) {
      if (!map.has(type)) map.set(type, [])
      map.get(type).push({ fn, options })
    },
    remove(type, fn) {
      const list = map.get(type)
      if (list) map.set(type, list.filter((l) => l.fn !== fn))
    },
    count: (type) => (map.get(type) || []).length,
    options: (type) => (map.get(type) || []).map((l) => l.options),
    fire(type, init = {}) {
      const event = makeEvent(type, init)
      for (const { fn } of [...(map.get(type) || [])]) fn(event)
      return event
    },
  }
}

/**
 * A fresh page running the script, with one `<video>` per spec, each in its
 * own player `<div>` followed by that player's own controls `<div>`.
 */
export function loadPopout({ videos = [{}], pip = false, subframe = false, channel = true, mediaSession = true } = {}) {
  const clock = createClock()
  const sent = []
  const page = {
    viewportWidth: 900,
    above: [],
    playRejects: false,
    pipRejects: false,
    pipThrows: false,
    channelThrows: false,
  }
  const root = makeNode(page, 'html')
  page.root = root
  const body = root.appendChild(makeNode(page, 'body'))

  const docEvents = registry()
  const winEvents = registry()
  const window = {
    addEventListener: winEvents.add,
    removeEventListener: winEvents.remove,
  }
  window.top = subframe ? {} : window
  if (channel) {
    window.webkit = {
      messageHandlers: {
        wisPopout: {
          postMessage(msg) {
            if (page.channelThrows) throw new Error('handler gone')
            sent.push(msg)
          },
        },
      },
    }
  }
  const actions = {}
  const navigator = mediaSession
    ? { mediaSession: { setActionHandler: (name, fn) => (actions[name] = [...(actions[name] || []), fn]) } }
    : {}

  const inside = (r, x, y) => x >= r.left && x <= r.right && y >= r.top && y <= r.bottom
  const document = {
    documentElement: root,
    body,
    pictureInPictureElement: null,
    pipExits: 0,
    exitPictureInPicture() {
      document.pipExits++
      return Promise.resolve()
    },
    addEventListener: docEvents.add,
    removeEventListener: docEvents.remove,
    createElement: (tag) => makeNode(page, tag),
    createElementNS: (ns, tag) => Object.assign(makeNode(page, tag), { namespaceURI: ns }),
    querySelectorAll(selector) {
      if (selector !== 'video') throw new Error(`unexpected selector ${selector}`)
      return walk(root).filter((n) => n.tagName === 'VIDEO')
    },
    // `page.above` holds things stacked over the page (popups, overlays);
    // the rest is tree order, later on top.
    elementsFromPoint(x, y) {
      const hit = (el) => !el.rect || inside(el.getBoundingClientRect(), x, y)
      const tree = walk(root)
        .reverse()
        .filter((el) => el.rect && hit(el))
      return [...page.above.filter(hit), ...tree, body, root]
    },
  }

  const install = () =>
    new Function(
      'window',
      'document',
      'navigator',
      'setTimeout',
      'clearTimeout',
      'setInterval',
      'clearInterval',
      'requestAnimationFrame',
      'cancelAnimationFrame',
      'Date',
      SCRIPT,
    )(
      window,
      document,
      navigator,
      clock.setTimeout,
      clock.clearTimeout,
      clock.setInterval,
      clock.clearInterval,
      clock.requestAnimationFrame,
      clock.cancelAnimationFrame,
      clock.Date,
    )

  const addVideo = (spec = {}) => {
    const player = body.appendChild(makeNode(page, 'div'))
    const video = player.appendChild(makeVideo(page, spec))
    player.appendChild(makeNode(page, 'div'))
    return video
  }
  const list = videos.map(addVideo)
  install()
  // Windows injects the flag after the script; it is read at call time.
  if (pip) evalIn(window, NATIVE.pipFlag)

  const byId = (id) => walk(root).find((n) => n.id === id) || null

  return {
    page,
    clock,
    sent,
    window,
    document,
    navigator,
    actions,
    root,
    body,
    videos: list,
    addVideo,
    install,
    docEvents,
    winEvents,
    byId,
    kinds: () => sent.map((m) => m.kind),

    enter: () => evalIn(window, NATIVE.enter),
    exit: () => evalIn(window, NATIVE.exit),
    resume: () => evalIn(window, NATIVE.resume),
    hover: (on) => evalIn(window, on ? NATIVE.hoverOn : NATIVE.hoverOff),
    enterPip: () => evalIn(window, NATIVE.enterPip),
    exitPip: () => evalIn(window, NATIVE.exitPip),

    /** A media event on `video`: document capture listeners first, then the video's own. */
    media(video, type) {
      docEvents.fire(type, { target: video })
      return video.fire(type)
    },
    /** Let a debounced media report run. */
    settle: () => clock.advance(120),
    move: (x, y) => docEvents.fire('mousemove', { clientX: x, clientY: y }),
    mouse(x, y) {
      docEvents.fire('mousemove', { clientX: x, clientY: y })
      clock.frame()
    },
    scroll: () => docEvents.fire('scroll', {}),
    hoverButton: () => byId('__wis-popout-hover'),

    /** The popout's controls while popped out, or null. */
    ui() {
      const overlay = byId('__wis-popout-overlay')
      if (!overlay) return null
      const child = (id) => overlay.children.find((c) => c.id === id)
      const vol = child('__wis-popout-vol')
      const seek = child('__wis-popout-seek')
      const volBtn = vol.children[0]
      const corners = Object.fromEntries(
        overlay.children.filter((c) => c.getAttribute('data-corner')).map((c) => [c.getAttribute('data-corner'), c]),
      )
      return {
        overlay,
        style: byId('__wis-popout-style'),
        x: child('__wis-popout-x'),
        paused: child('__wis-popout-paused'),
        badge: child('__wis-popout-badge'),
        vol,
        volBtn,
        volOn: volBtn.children[0],
        volOff: volBtn.children[1],
        volSlider: vol.children[1],
        seek,
        seekSlider: seek.children[0],
        corners,
      }
    },
  }
}
