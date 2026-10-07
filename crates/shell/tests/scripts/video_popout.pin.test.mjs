// The video popout script's pin and restore round trip, its watchdog, and the playback kick after a window move.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { loadPopout } from '../support/s3-popoutPage.mjs'

const POPOUT_CLASSES = { root: '__wis-popout-mode', video: '__wis-popout-video' }

/** A page with one video playing in its player, between nothing and the player's controls. */
function playing(spec = {}) {
  return loadPopout({ videos: [{ playing: true, ...spec }] })
}

describe('entering', () => {
  test('the video is pinned under the root over a hidden body, and native gets its aspect', () => {
    const p = playing({ videoWidth: 1280, videoHeight: 720 })
    const [v] = p.videos
    p.enter()
    const ui = p.ui()
    assert.ok(p.root.classList.contains(POPOUT_CLASSES.root))
    assert.ok(v.classList.contains(POPOUT_CLASSES.video))
    assert.deepEqual(p.root.children.slice(-3), [ui.style, v, ui.overlay])
    assert.match(ui.style.textContent, /html\.__wis-popout-mode body\{visibility:hidden!important/)
    assert.match(ui.style.textContent, /video\.__wis-popout-video\{position:fixed!important/)
    assert.deepEqual(p.sent, [{ kind: 'entered', aspect: 1280 / 720 }])
  })

  test('the overlay carries the four resize corners native understands, X, volume and seek', () => {
    const p = playing()
    p.enter()
    const ui = p.ui()
    assert.deepEqual(Object.keys(ui.corners), ['nw', 'ne', 'sw', 'se'])
    assert.equal(ui.x.textContent, '×')
    assert.equal(ui.volSlider.type, 'range')
    assert.equal(ui.volSlider.getAttribute('aria-label'), 'Volume')
    assert.equal(ui.seekSlider.getAttribute('aria-label'), 'Seek')
  })

  test('the controls start highlighted and settle after 1800ms', () => {
    const p = playing()
    p.enter()
    const { overlay } = p.ui()
    assert.ok(overlay.classList.contains('is-fresh'))
    p.clock.advance(1799)
    assert.ok(overlay.classList.contains('is-fresh'))
    p.clock.advance(1)
    assert.ok(!overlay.classList.contains('is-fresh'))
  })

  test('a second enter while popped out does nothing', () => {
    const p = playing()
    p.enter()
    p.enter()
    assert.deepEqual(p.kinds(), ['entered'])
    assert.equal(p.root.children.filter((c) => c.id === '__wis-popout-overlay').length, 1)
  })
})

describe('exiting', () => {
  test('the video goes back to its original slot and the page styling unwinds, without a message', () => {
    const p = playing()
    const [v] = p.videos
    const player = v.parentNode
    const [, controls] = player.children
    p.enter()
    const { style, overlay } = p.ui()
    p.exit()
    assert.deepEqual(player.children, [v, controls])
    assert.ok(!v.classList.contains(POPOUT_CLASSES.video))
    assert.ok(!p.root.classList.contains(POPOUT_CLASSES.root))
    assert.ok(!style.isConnected && !overlay.isConnected)
    assert.deepEqual(p.kinds(), ['entered'])
  })

  test('a video whose next sibling has gone is appended back to its original parent', () => {
    const p = playing()
    const [v] = p.videos
    const player = v.parentNode
    const lead = player.insertBefore(p.document.createElement('div'), v)
    p.enter()
    player.children[1].remove()
    p.exit()
    assert.deepEqual(player.children, [lead, v])
  })

  test('a video whose next sibling moved elsewhere is appended back to its original parent', () => {
    const p = playing()
    const [v] = p.videos
    const player = v.parentNode
    const controls = player.children[1]
    p.enter()
    p.body.appendChild(controls)
    p.exit()
    assert.equal(v.parentNode, player)
    assert.equal(player.children.at(-1), v)
  })

  test('a video whose original container left the page is left where it is, unstyled', () => {
    const p = playing()
    const [v] = p.videos
    const player = v.parentNode
    p.enter()
    player.remove()
    p.exit()
    assert.equal(v.parentNode, p.root)
    assert.ok(!v.classList.contains(POPOUT_CLASSES.video))
    assert.equal(p.ui(), null)
  })

  test('exit with nothing popped out does nothing', () => {
    const p = playing()
    p.exit()
    assert.deepEqual(p.sent, [])
    assert.equal(p.videos[0].parentNode.parentNode, p.body)
  })

  test('exit stops the watchdog and detaches every listener the popout added', () => {
    const p = playing()
    const [v] = p.videos
    p.enter()
    const types = ['play', 'pause', 'resize', 'volumechange', 'timeupdate', 'seeked', 'error', 'durationchange', 'loadedmetadata', 'emptied']
    assert.ok(types.every((t) => v.listenerCount(t) === 1))
    assert.equal(p.winEvents.count('pagehide'), 1)
    p.exit()
    assert.ok(types.every((t) => v.listenerCount(t) === 0))
    assert.equal(p.winEvents.count('pagehide'), 0)
    v.remove()
    p.clock.advance(3000)
    assert.deepEqual(p.kinds(), ['entered'])
  })

  test('a popout can be entered again after an exit', () => {
    const p = playing()
    p.enter()
    p.exit()
    p.enter()
    assert.deepEqual(p.kinds(), ['entered', 'entered'])
    assert.ok(p.videos[0].classList.contains(POPOUT_CLASSES.video))
  })
})

describe('while popped out', () => {
  test('the watchdog reports a pinned video the page removed as lost, and unwinds', () => {
    const p = playing()
    const [v] = p.videos
    p.enter()
    v.remove()
    p.clock.advance(1499)
    assert.deepEqual(p.kinds(), ['entered'])
    p.clock.advance(1)
    assert.deepEqual(p.kinds(), ['entered', 'lost'])
    assert.ok(!p.root.classList.contains(POPOUT_CLASSES.root))
    assert.equal(p.ui(), null)
    p.clock.advance(3000)
    assert.deepEqual(p.kinds(), ['entered', 'lost'], 'once')
  })

  test('the watchdog puts back styling and controls the page stripped', () => {
    const p = playing()
    const [v] = p.videos
    p.enter()
    const { style, overlay } = p.ui()
    style.remove()
    overlay.remove()
    p.root.classList.remove(POPOUT_CLASSES.root)
    v.classList.remove(POPOUT_CLASSES.video)
    p.clock.advance(1500)
    assert.ok(style.isConnected && overlay.isConnected)
    assert.ok(p.root.classList.contains(POPOUT_CLASSES.root))
    assert.ok(v.classList.contains(POPOUT_CLASSES.video))
    assert.deepEqual(p.kinds(), ['entered'])
  })

  test('pagehide unwinds and reports lost', () => {
    const p = playing()
    p.enter()
    p.winEvents.fire('pagehide')
    assert.deepEqual(p.kinds(), ['entered', 'lost'])
    assert.equal(p.ui(), null)
  })

  test('the shell hover signal toggles the hover look, and is ignored when not popped out', () => {
    const p = playing()
    p.hover(true)
    p.enter()
    const { overlay } = p.ui()
    assert.ok(!overlay.classList.contains('is-hover'))
    p.hover(true)
    assert.ok(overlay.classList.contains('is-hover'))
    p.hover(false)
    assert.ok(!overlay.classList.contains('is-hover'))
  })
})

describe('resuming after a window move', () => {
  test('a video playing at enter is kicked back into playback when the move paused it', () => {
    const p = playing()
    const [v] = p.videos
    p.enter()
    v.paused = true
    p.resume()
    assert.equal(v.plays, 1)
    assert.equal(v.paused, false)
  })

  test('the kick retries every 200ms for 1200ms while the engine keeps pausing', () => {
    const p = playing()
    const [v] = p.videos
    p.enter()
    v.play = () => {
      v.plays++
      return Promise.resolve()
    }
    v.paused = true
    p.resume()
    p.clock.advance(1199)
    assert.equal(v.plays, 6)
    p.clock.advance(1)
    assert.equal(v.plays, 7)
    p.clock.advance(5000)
    assert.equal(v.plays, 7)
  })

  test('the kick only plays a paused video, and keeps watching for a late pause', () => {
    const p = playing()
    const [v] = p.videos
    p.enter()
    p.resume()
    assert.equal(v.plays, 0)
    p.clock.advance(400)
    v.paused = true
    p.clock.advance(200)
    assert.equal(v.plays, 1)
  })

  test('a resume is used once', () => {
    const p = playing()
    const [v] = p.videos
    p.enter()
    p.resume()
    p.clock.advance(1200)
    v.paused = true
    p.resume()
    p.clock.advance(1200)
    assert.equal(v.plays, 0)
  })

  test('a video paused at enter is not resumed', () => {
    const p = loadPopout({ videos: [{}] })
    p.enter()
    p.resume()
    assert.equal(p.videos[0].plays, 0)
  })

  test('a pause by the user in the popout stops the kicks', () => {
    const p = playing()
    const [v] = p.videos
    p.enter()
    p.resume()
    const { overlay } = p.ui()
    overlay.fire('pointerdown', { button: 0, screenX: 450, screenY: 250, clientX: 450, clientY: 250, pointerId: 1 })
    overlay.fire('pointerup', { button: 0, screenX: 450, screenY: 250, clientX: 450, clientY: 250, pointerId: 1 })
    p.clock.advance(300)
    assert.equal(v.paused, true)
    p.clock.advance(1200)
    assert.equal(v.plays, 0)
  })

  test('a video that ended or left the page is not kicked', () => {
    const ended = playing()
    ended.enter()
    Object.assign(ended.videos[0], { paused: true, ended: true })
    ended.resume()
    ended.clock.advance(1200)
    assert.equal(ended.videos[0].plays, 0)

    const gone = playing()
    gone.enter()
    gone.videos[0].paused = true
    gone.videos[0].remove()
    gone.resume()
    gone.clock.advance(1200)
    assert.equal(gone.videos[0].plays, 0)
  })

  test('closing with the X never resumes', () => {
    const p = playing()
    const [v] = p.videos
    p.enter()
    p.ui().x.fire('click')
    p.resume()
    p.clock.advance(1200)
    assert.equal(v.plays, 0)
  })

  test('exit records whether the video plays, so the move back can resume it', () => {
    const p = loadPopout({ videos: [{}] })
    const [v] = p.videos
    p.enter()
    v.paused = false
    p.exit()
    v.paused = true
    p.resume()
    assert.equal(v.plays, 1)

    const q = playing()
    q.enter()
    q.videos[0].paused = true
    q.exit()
    q.resume()
    assert.equal(q.videos[0].plays, 0, 'paused at exit')
  })

  test('a refused play is swallowed', () => {
    const p = playing()
    p.page.playRejects = true
    p.enter()
    p.videos[0].paused = true
    p.resume()
    assert.equal(p.videos[0].plays, 1)
  })
})
