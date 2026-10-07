// The video popout overlay's controls: close, click and double click, drag and corner resize, volume, the seek scrubber, and media changes while popped out.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { rustHas } from '../support/s3-native.mjs'
import { loadPopout } from '../support/s3-popoutPage.mjs'

/** A popped-out page: one 10-minute video playing at 100s, volume 0.6; the entered message is cleared. */
function popped(spec = {}) {
  const p = loadPopout({ videos: [{ playing: true, currentTime: 100, volume: 0.6, ...spec }] })
  p.enter()
  p.sent.length = 0
  return { p, v: p.videos[0], ui: p.ui() }
}

/** A pointer event on the overlay; the overlay is 900px wide, so its thirds end at 300 and 600. */
function pointer(ui, type, init = {}) {
  const x = init.x ?? 450
  const y = init.y ?? 250
  return ui.overlay.fire(type, {
    target: ui.overlay,
    button: 0,
    pointerId: 7,
    clientX: x,
    clientY: y,
    screenX: x,
    screenY: y,
    ...init,
  })
}

function click(ui, x = 450, target = ui.overlay) {
  pointer(ui, 'pointerdown', { x, target })
  pointer(ui, 'pointerup', { x, target })
}

const near = (actual, expected) => assert.ok(Math.abs(actual - expected) < 1e-9, `${actual} is not ${expected}`)

describe('the X button', () => {
  test('it pauses the video, unwinds the popout and reports close', () => {
    const { p, v, ui } = popped()
    const player = p.body.children[0]
    const event = ui.x.fire('click')
    assert.ok(event.defaultPrevented && event.propagationStopped)
    assert.equal(v.pauses, 1)
    assert.deepEqual(p.kinds(), ['close'])
    assert.equal(p.ui(), null)
    assert.equal(v.parentNode, player)
  })

  test('a click the page dispatched itself does nothing', () => {
    const { p, v, ui } = popped()
    ui.x.fire('click', { isTrusted: false })
    assert.equal(v.pauses, 0)
    assert.deepEqual(p.kinds(), [])
    assert.notEqual(p.ui(), null)
  })
})

describe('clicks on the video', () => {
  test('a single click toggles playback after 300ms', () => {
    const { p, v, ui } = popped()
    click(ui)
    p.clock.advance(299)
    assert.equal(v.paused, false)
    p.clock.advance(1)
    assert.equal(v.paused, true)
    click(ui)
    p.clock.advance(300)
    assert.equal(v.paused, false)
    assert.equal(v.plays, 1)
  })

  test('the paused badge follows play and pause', () => {
    const { p, v, ui } = popped()
    assert.equal(ui.paused.style.opacity, '0')
    v.paused = true
    p.media(v, 'pause')
    assert.equal(ui.paused.style.opacity, '1')
    v.paused = false
    p.media(v, 'play')
    assert.equal(ui.paused.style.opacity, '0')

    const { ui: stopped } = popped({ playing: false })
    assert.equal(stopped.paused.style.opacity, '1')
  })

  test('a double click on the left third seeks back 10s and flashes -10s, without a toggle', () => {
    const { p, v, ui } = popped()
    click(ui, 100)
    p.clock.advance(100)
    click(ui, 100)
    assert.equal(v.currentTime, 90)
    assert.equal(ui.badge.textContent, '-10s')
    assert.ok(ui.badge.classList.contains('is-on'))
    p.clock.advance(1000)
    assert.equal(v.paused, false)
  })

  test('a double click on the right third seeks forward 10s and flashes +10s', () => {
    const { v, ui } = popped()
    click(ui, 800)
    click(ui, 800)
    assert.equal(v.currentTime, 110)
    assert.equal(ui.badge.textContent, '+10s')
  })

  test('a double click in the middle third toggles at once', () => {
    const { p, v, ui } = popped()
    click(ui)
    click(ui)
    assert.equal(v.paused, true)
    p.clock.advance(1000)
    assert.equal(v.pauses, 1)
  })

  test('the thirds split at a third and two thirds of the overlay width', () => {
    const at = (x) => {
      const { v, ui } = popped()
      click(ui, x)
      click(ui, x)
      return v.paused ? 'toggle' : v.currentTime - 100
    }
    assert.equal(at(299), -10)
    assert.equal(at(300), 'toggle')
    assert.equal(at(600), 'toggle')
    assert.equal(at(601), 10)
  })

  test('a click after a double click starts a new single click', () => {
    const { p, v, ui } = popped()
    click(ui, 100)
    click(ui, 100)
    click(ui)
    p.clock.advance(300)
    assert.equal(v.paused, true)
  })

  test('seeks clamp to the start and to the duration, and live streams have no end', () => {
    const start = popped({ currentTime: 4 })
    click(start.ui, 100)
    click(start.ui, 100)
    assert.equal(start.v.currentTime, 0)

    const end = popped({ currentTime: 595 })
    click(end.ui, 800)
    click(end.ui, 800)
    assert.equal(end.v.currentTime, 600)

    const live = popped({ duration: Infinity, currentTime: 5000 })
    click(live.ui, 800)
    click(live.ui, 800)
    assert.equal(live.v.currentTime, 5010)
  })

  test('a seek the engine refuses flashes nothing', () => {
    const { v, ui } = popped()
    v.refuseSeek = true
    click(ui, 100)
    click(ui, 100)
    assert.ok(!ui.badge.textContent)
    assert.ok(!ui.badge.classList.contains('is-on'))
  })

  test('a press captures the pointer and keeps the page from acting on it', () => {
    const { ui } = popped()
    const down = pointer(ui, 'pointerdown')
    assert.equal(ui.overlay.capturedPointer, 7)
    assert.ok(down.defaultPrevented)
  })

  test('only the primary button presses, and a release with no press is ignored', () => {
    const { p, v, ui } = popped()
    pointer(ui, 'pointerdown', { button: 2 })
    pointer(ui, 'pointerup', { button: 2 })
    pointer(ui, 'pointerup')
    p.clock.advance(300)
    assert.equal(v.paused, false)
    assert.equal(ui.overlay.capturedPointer, undefined)
  })

  test('presses on the X, the volume pill or the scrubber are left to those controls', () => {
    const { p, v, ui } = popped()
    for (const target of [ui.x, ui.volBtn, ui.volSlider, ui.seekSlider]) click(ui, 450, target)
    p.clock.advance(300)
    assert.equal(v.paused, false)
    assert.equal(ui.overlay.capturedPointer, undefined)
  })

  test('a cancelled press is not a click', () => {
    const { p, v, ui } = popped()
    pointer(ui, 'pointerdown')
    pointer(ui, 'pointercancel')
    pointer(ui, 'pointerup')
    p.clock.advance(300)
    assert.equal(v.paused, false)
  })

  test('the context menu is suppressed', () => {
    const { ui } = popped()
    assert.ok(ui.overlay.fire('contextmenu').defaultPrevented)
  })
})

describe('drag and corner resize', () => {
  test('a drag or corner resize the page dispatched itself reaches neither native nor the video', () => {
    const { p, v, ui } = popped()
    pointer(ui, 'pointerdown', { x: 450, y: 250, isTrusted: false })
    pointer(ui, 'pointermove', { x: 500, y: 300, isTrusted: false })
    pointer(ui, 'pointerup', { x: 500, y: 300, isTrusted: false })
    p.clock.advance(300)
    assert.deepEqual(p.sent, [])
    assert.equal(v.paused, false)

    pointer(ui, 'pointerdown', { x: 450, y: 250 })
    pointer(ui, 'pointermove', { x: 500, y: 300, isTrusted: false })
    assert.deepEqual(p.sent, [], 'a forged move during a real press')
  })

  test('dragging the video past 4px hands the window move to native once, and is not a click', () => {
    const { p, v, ui } = popped()
    pointer(ui, 'pointermove', { x: 500 })
    assert.deepEqual(p.sent, [], 'a move with no press')
    pointer(ui, 'pointerdown', { x: 450, y: 250 })
    pointer(ui, 'pointermove', { x: 453, y: 247 })
    assert.deepEqual(p.sent, [])
    pointer(ui, 'pointermove', { x: 454, y: 250 })
    assert.deepEqual(p.sent, [{ kind: 'dragstart' }], 'at 4px')
    pointer(ui, 'pointermove', { x: 500, y: 300 })
    pointer(ui, 'pointerup', { x: 500, y: 300 })
    p.clock.advance(300)
    assert.deepEqual(p.sent, [{ kind: 'dragstart' }])
    assert.equal(v.paused, false)
  })

  test('dragging a corner reports resize deltas once per frame, after the 4px slop', () => {
    const { p, ui } = popped()
    const se = ui.corners.se
    pointer(ui, 'pointerdown', { target: se, x: 800, y: 500 })
    pointer(ui, 'pointermove', { x: 806, y: 500 })
    p.clock.frame()
    assert.deepEqual(p.sent, [], 'the slop is not counted')
    pointer(ui, 'pointermove', { x: 816, y: 503 })
    pointer(ui, 'pointermove', { x: 820, y: 505 })
    assert.equal(p.clock.pendingFrames(), 1)
    p.clock.frame()
    assert.deepEqual(p.sent, [{ kind: 'resize', corner: 'se', dx: 14, dy: 5 }])
    p.clock.frame()
    pointer(ui, 'pointermove', { x: 818, y: 505 })
    p.clock.frame()
    assert.deepEqual(p.sent.at(-1), { kind: 'resize', corner: 'se', dx: -2, dy: 0 })
    assert.equal(p.sent.length, 2)
  })

  test('releasing a corner sends the movement not yet reported and drops the pending frame', () => {
    const { p, ui } = popped()
    pointer(ui, 'pointerdown', { target: ui.corners.nw, x: 10, y: 10 })
    pointer(ui, 'pointermove', { x: 5, y: 5 })
    pointer(ui, 'pointermove', { x: -5, y: 5 })
    pointer(ui, 'pointerup', { x: -5, y: 5 })
    assert.deepEqual(p.sent, [{ kind: 'resize', corner: 'nw', dx: -10, dy: 0 }])
    p.clock.frame()
    assert.equal(p.sent.length, 1)
  })

  test('releasing a corner with nothing pending sends nothing more', () => {
    const { p, ui } = popped()
    pointer(ui, 'pointerdown', { target: ui.corners.ne, x: 800, y: 10 })
    pointer(ui, 'pointermove', { x: 810, y: 10 })
    pointer(ui, 'pointermove', { x: 820, y: 10 })
    p.clock.frame()
    pointer(ui, 'pointerup', { x: 820, y: 10 })
    assert.deepEqual(p.sent, [{ kind: 'resize', corner: 'ne', dx: 10, dy: 0 }])
  })

  test('a corner press without movement is a click', () => {
    const { p, v, ui } = popped()
    click(ui, 890, ui.corners.sw)
    p.clock.advance(300)
    assert.equal(v.paused, true)
    assert.deepEqual(p.sent, [])
  })

  test('every corner zone is named as native parses it', () => {
    const { ui } = popped()
    for (const corner of Object.keys(ui.corners)) {
      assert.ok(rustHas('video_popout/geometry.rs', `"${corner}" => Some`), corner)
    }
  })
})

describe('volume', () => {
  test('the volume pill mirrors the element: icon, label and slider', () => {
    const { p, v, ui } = popped()
    assert.deepEqual([ui.volOn.style.display, ui.volOff.style.display], ['block', 'none'])
    assert.equal(ui.volBtn.title, 'Mute')
    assert.equal(ui.volBtn.getAttribute('aria-label'), 'Mute')
    assert.equal(ui.volSlider.value, '0.6')

    v.muted = true
    p.media(v, 'volumechange')
    assert.deepEqual([ui.volOn.style.display, ui.volOff.style.display], ['none', 'block'])
    assert.equal(ui.volBtn.title, 'Unmute')
    assert.equal(ui.volBtn.getAttribute('aria-label'), 'Unmute')

    Object.assign(v, { muted: false, volume: 0 })
    p.media(v, 'volumechange')
    assert.equal(ui.volOff.style.display, 'block', 'silent at zero volume')
    assert.equal(ui.volBtn.title, 'Mute')
    assert.equal(ui.volSlider.value, '0')
  })

  test('the mute button toggles mute without reaching the overlay', () => {
    const { v, ui } = popped()
    const event = ui.volBtn.fire('click')
    assert.equal(v.muted, true)
    assert.ok(event.defaultPrevented && event.propagationStopped)
    ui.volBtn.fire('click')
    assert.equal(v.muted, false)
  })

  test('the slider sets the volume clamped to 0..1, and unmutes only above zero', () => {
    const { v, ui } = popped({ muted: true })
    const slide = (value) => {
      ui.volSlider.value = value
      ui.volSlider.fire('input')
    }
    slide('0')
    assert.deepEqual([v.volume, v.muted], [0, true])
    slide('0.4')
    assert.deepEqual([v.volume, v.muted], [0.4, false])
    slide('1.5')
    assert.equal(v.volume, 1)
    slide('-1')
    assert.equal(v.volume, 0)
    slide('loud')
    assert.equal(v.volume, 0)
  })

  test('the slider is not overwritten while it is being dragged', () => {
    const { p, v, ui } = popped()
    for (const release of ['pointerup', 'pointercancel']) {
      ui.volSlider.fire('pointerdown')
      v.volume = 0.2
      p.media(v, 'volumechange')
      assert.equal(ui.volSlider.value, '0.6', release)
      ui.volSlider.fire(release)
      p.media(v, 'volumechange')
      assert.equal(ui.volSlider.value, '0.2', release)
      v.volume = 0.6
      p.media(v, 'volumechange')
    }
  })

  test('the wheel over the volume pill steps the volume by 5%, clamped, unmuting', () => {
    const { v, ui } = popped({ muted: true })
    const wheel = (deltaY) => ui.overlay.fire('wheel', { target: ui.volSlider, deltaY })
    assert.ok(wheel(-1).defaultPrevented)
    near(v.volume, 0.65)
    assert.equal(v.muted, false)
    wheel(3)
    near(v.volume, 0.6)
    v.volume = 0.98
    wheel(-1)
    assert.equal(v.volume, 1)
    v.volume = 0.02
    wheel(1)
    assert.equal(v.volume, 0)
  })

  test('the wheel elsewhere on the overlay is swallowed without a change', () => {
    const { v, ui } = popped()
    const event = ui.overlay.fire('wheel', { target: ui.overlay, deltaY: -1 })
    assert.ok(event.defaultPrevented)
    assert.equal(v.volume, 0.6)
  })
})

describe('the seek scrubber', () => {
  const slide = (ui, value) => {
    ui.seekSlider.value = value
    ui.seekSlider.fire('input')
  }

  test('it shows for a finite duration, and hides for live streams and before metadata', () => {
    assert.equal(popped().ui.seek.style.display, 'flex')
    assert.equal(popped({ duration: Infinity }).ui.seek.style.display, 'none')
    const { v, ui } = popped({ duration: NaN })
    assert.equal(ui.seek.style.display, 'none')
    v.duration = 300
    v.fire('durationchange')
    assert.equal(ui.seek.style.display, 'flex')
    assert.equal(ui.seekSlider.value, String(100 / 300))
  })

  test('it follows playback', () => {
    const { p, v, ui } = popped()
    assert.equal(ui.seekSlider.value, String(100 / 600))
    v.currentTime = 150
    p.media(v, 'timeupdate')
    assert.equal(ui.seekSlider.value, '0.25')
  })

  test('it does not follow while dragged, while the engine seeks, or while a seek is queued', () => {
    const dragged = popped()
    dragged.ui.seekSlider.fire('pointerdown')
    dragged.v.currentTime = 300
    dragged.v.fire('timeupdate')
    assert.equal(dragged.ui.seekSlider.value, String(100 / 600))

    const seeking = popped()
    seeking.v.seeking = true
    seeking.v.currentTime = 300
    seeking.v.fire('timeupdate')
    assert.equal(seeking.ui.seekSlider.value, String(100 / 600))

    const queued = popped()
    slide(queued.ui, '0.1')
    slide(queued.ui, '0.9')
    queued.v.fire('timeupdate')
    assert.equal(queued.ui.seekSlider.value, '0.9')
  })

  test('a drag scrubs with fastSeek, and the release lands exactly', () => {
    const { p, v, ui } = popped()
    ui.seekSlider.fire('pointerdown')
    slide(ui, '0.5')
    assert.deepEqual(v.fastSeeks, [300])
    ui.seekSlider.fire('pointerup')
    assert.deepEqual(v.seeks, [])
    p.clock.advance(180)
    assert.deepEqual(v.seeks, [300])
  })

  test('a drag stops short of the end until release, by at most a quarter second', () => {
    const { p, v, ui } = popped()
    ui.seekSlider.fire('pointerdown')
    slide(ui, '1')
    assert.deepEqual(v.fastSeeks, [599.75])
    ui.seekSlider.fire('pointercancel')
    p.clock.advance(180)
    assert.deepEqual(v.seeks, [600])

    const clip = popped({ duration: 2, currentTime: 0 })
    clip.ui.seekSlider.fire('pointerdown')
    slide(clip.ui, '1')
    near(clip.v.fastSeeks[0], 1.8)
  })

  test('without fastSeek a drag sets currentTime', () => {
    const { v, ui } = popped()
    delete v.fastSeek
    ui.seekSlider.fire('pointerdown')
    slide(ui, '0.5')
    assert.deepEqual(v.seeks, [300])
  })

  test('seeks are spaced 180ms apart and only the newest target survives', () => {
    const { p, v, ui } = popped()
    slide(ui, '0.1')
    p.clock.advance(50)
    slide(ui, '0.2')
    slide(ui, '0.3')
    p.clock.advance(129)
    assert.deepEqual(v.seeks, [60])
    p.clock.advance(1)
    assert.deepEqual(v.seeks, [60, 180])
  })

  test('out-of-range values clamp to the ends', () => {
    const { p, v, ui } = popped()
    slide(ui, '1.5')
    p.clock.advance(180)
    slide(ui, '-0.5')
    assert.deepEqual(v.seeks, [600, 0])
  })

  test('while the engine is still seeking the next seek waits, for at most 800ms', () => {
    const { p, v, ui } = popped()
    slide(ui, '0.1')
    v.seeking = true
    p.clock.advance(200)
    slide(ui, '0.2')
    p.clock.advance(599)
    assert.deepEqual(v.seeks, [60])
    p.clock.advance(1)
    assert.deepEqual(v.seeks, [60, 120])
  })

  test('a finished seek lets a queued one go at once', () => {
    const { p, v, ui } = popped()
    slide(ui, '0.1')
    v.seeking = true
    p.clock.advance(300)
    slide(ui, '0.2')
    p.clock.advance(10)
    v.seeking = false
    v.fire('seeked')
    assert.deepEqual(v.seeks, [60, 120])
    assert.equal(ui.seekSlider.value, '0.2')
    p.clock.advance(1000)
    assert.deepEqual(v.seeks, [60, 120])
  })

  test('a source change drops a queued seek, a duration change keeps it', () => {
    for (const [event, kept] of [
      ['emptied', false],
      ['loadedmetadata', false],
      ['durationchange', true],
    ]) {
      const { p, v, ui } = popped()
      slide(ui, '0.1')
      slide(ui, '0.2')
      v.fire(event)
      p.clock.advance(1000)
      assert.deepEqual(v.seeks, kept ? [60, 120] : [60], event)
    }
  })

  test('input is ignored for live streams and for values that are not numbers', () => {
    const live = popped({ duration: Infinity })
    slide(live.ui, '0.5')
    assert.deepEqual(live.v.seeks, [])

    const { v, ui } = popped()
    slide(ui, 'half')
    assert.deepEqual(v.seeks, [])
  })
})

describe('media changes while popped out', () => {
  test('a video resize reports the new aspect after 150ms, once per burst', () => {
    const { p, v } = popped()
    Object.assign(v, { videoWidth: 1440, videoHeight: 1080 })
    v.fire('resize')
    p.clock.advance(100)
    v.fire('resize')
    p.clock.advance(149)
    assert.deepEqual(p.sent, [])
    p.clock.advance(1)
    assert.deepEqual(p.sent, [{ kind: 'aspect', aspect: 1440 / 1080 }])
  })

  test('a resize with no video size reports nothing, and one pending at exit is dropped', () => {
    const sizeless = popped()
    sizeless.v.videoHeight = 0
    sizeless.v.fire('resize')
    sizeless.p.clock.advance(150)
    assert.deepEqual(sizeless.p.sent, [])

    const { p, v } = popped()
    v.fire('resize')
    p.exit()
    p.clock.advance(150)
    assert.deepEqual(p.sent, [])
  })

  test('a fatal media error unwinds and reports lost, an error event without one does nothing', () => {
    const { p, v } = popped()
    v.fire('error')
    assert.deepEqual(p.sent, [])
    v.error = { code: 3 }
    v.fire('error')
    assert.deepEqual(p.kinds(), ['lost'])
    assert.equal(p.ui(), null)
  })
})
