// How the video popout script installs, reports media state to native, and chooses which video to pin.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { loadPopout } from '../support/s3-popoutPage.mjs'

const pinned = (video) => video.classList.contains('__wis-popout-video')

describe('install', () => {
  test('the script installs its hooks once per page', () => {
    const p = loadPopout()
    const enter = p.window.__wisPopoutEnter
    p.install()
    assert.equal(p.window.__wisPopoutEnter, enter)
    assert.equal(p.docEvents.count('play'), 1)
    assert.equal(p.docEvents.count('mousemove'), 1)
  })

  test('a subframe installs nothing', () => {
    const p = loadPopout({ subframe: true })
    assert.equal(p.window.__wisPopoutEnter, undefined)
    assert.equal(p.window.__wisEnterPip, undefined)
    assert.equal(p.docEvents.count('play'), 0)
  })

  test('media reports listen on the document in the capture phase', () => {
    const p = loadPopout()
    for (const type of ['play', 'playing', 'pause', 'ended', 'volumechange', 'emptied', 'loadedmetadata']) {
      assert.deepEqual(p.docEvents.options(type), [true], type)
    }
  })

  test('a page without the wisPopout channel, or with a failing one, reports nothing and does not throw', () => {
    const p = loadPopout({ channel: false, videos: [{ playing: true }] })
    p.media(p.videos[0], 'play')
    p.settle()
    p.enter()
    assert.ok(pinned(p.videos[0]))

    const q = loadPopout({ videos: [{ playing: true }] })
    q.page.channelThrows = true
    q.media(q.videos[0], 'play')
    q.settle()
    assert.deepEqual(q.sent, [])
  })
})

describe('media reports', () => {
  test('a large unmuted playing video reports playing and present', () => {
    const p = loadPopout({ videos: [{ playing: true }] })
    p.media(p.videos[0], 'play')
    p.settle()
    assert.deepEqual(p.sent, [{ kind: 'media', playing: true, present: true }])
  })

  test('a burst of media events within 120ms is reported once, with the state at the end', () => {
    const p = loadPopout({ videos: [{ playing: true }] })
    const [v] = p.videos
    p.media(v, 'play')
    p.clock.advance(60)
    v.paused = true
    p.media(v, 'pause')
    p.clock.advance(59)
    assert.deepEqual(p.sent, [])
    p.clock.advance(1)
    assert.deepEqual(p.sent, [{ kind: 'media', playing: false, present: true }])
  })

  test('an unchanged state is not reported again', () => {
    const p = loadPopout({ videos: [{ playing: true }] })
    p.media(p.videos[0], 'play')
    p.settle()
    p.media(p.videos[0], 'volumechange')
    p.settle()
    assert.equal(p.sent.length, 1)
  })

  test('pausing reports the video still present but not playing', () => {
    const p = loadPopout({ videos: [{ playing: true }] })
    const [v] = p.videos
    p.media(v, 'play')
    p.settle()
    v.paused = true
    p.media(v, 'pause')
    p.settle()
    assert.deepEqual(p.sent.at(-1), { kind: 'media', playing: false, present: true })
  })

  test('muted or ended videos never count as playing', () => {
    for (const spec of [{ muted: true }, { ended: true }]) {
      const p = loadPopout({ videos: [{ playing: true, ...spec }] })
      p.media(p.videos[0], 'playing')
      p.settle()
      assert.deepEqual(p.sent, [{ kind: 'media', playing: false, present: true }], JSON.stringify(spec))
    }
  })

  test('a video counts from 180x100 with metadata, and not below or without it', () => {
    const present = (spec) => {
      const p = loadPopout({ videos: [{ playing: true, ...spec }] })
      p.media(p.videos[0], 'play')
      p.settle()
      return p.sent.length > 0
    }
    assert.equal(present({ width: 180, height: 100 }), true)
    assert.equal(present({ width: 179, height: 100 }), false)
    assert.equal(present({ width: 180, height: 99 }), false)
    assert.equal(present({ videoWidth: 0 }), false)
    assert.equal(present({ videoHeight: 0 }), false)
  })

  test('a removed video is reported gone at the next media event', () => {
    const p = loadPopout({ videos: [{ playing: true }] })
    const [v] = p.videos
    p.media(v, 'play')
    p.settle()
    v.parentNode.remove()
    p.media(v, 'pause')
    p.settle()
    assert.deepEqual(p.sent.at(-1), { kind: 'media', playing: false, present: false })
  })

  test('a playing video elsewhere keeps the page playing when another pauses', () => {
    const p = loadPopout({ videos: [{ playing: true }, { playing: true, width: 300, height: 200 }] })
    const [big] = p.videos
    p.media(big, 'play')
    p.settle()
    big.paused = true
    p.media(big, 'pause')
    p.settle()
    assert.deepEqual(p.sent, [{ kind: 'media', playing: true, present: true }])
  })
})

describe('choosing the video to pin', () => {
  test('a playing video wins over a larger paused one', () => {
    const p = loadPopout({ videos: [{ width: 1280, height: 720 }, { playing: true }] })
    p.enter()
    assert.ok(pinned(p.videos[1]))
    assert.ok(!pinned(p.videos[0]))
  })

  test('among playing videos the largest wins, and muted ones are passed over', () => {
    const p = loadPopout({
      videos: [{ playing: true }, { playing: true, width: 800, height: 450 }, { playing: true, muted: true, width: 1280, height: 720 }],
    })
    p.enter()
    assert.deepEqual(p.videos.map(pinned), [false, true, false])
  })

  test('with nothing playing, the last video seen playing is pinned even after it shrank', () => {
    const p = loadPopout({ videos: [{ playing: true }, { width: 800, height: 450 }] })
    const [last, other] = p.videos
    p.media(last, 'play')
    p.settle()
    last.paused = true
    last.rect = { left: 0, top: 0, width: 100, height: 50 }
    p.enter()
    assert.ok(pinned(last))
    assert.ok(!pinned(other))
  })

  test('a last-seen video that left the page is passed over for the largest paused one', () => {
    const p = loadPopout({ videos: [{ playing: true }, { width: 300, height: 200 }, { width: 800, height: 450 }] })
    const [last, , largest] = p.videos
    p.media(last, 'play')
    p.settle()
    last.parentNode.remove()
    p.enter()
    assert.ok(pinned(largest))
  })

  test('with nothing playing or seen playing, the largest paused video is pinned', () => {
    const p = loadPopout({ videos: [{ width: 300, height: 200 }, { width: 800, height: 450 }] })
    p.enter()
    assert.deepEqual(p.videos.map(pinned), [false, true])
  })

  test('with no usable video, enter does nothing', () => {
    const p = loadPopout({ videos: [{ width: 100, height: 60 }] })
    p.enter()
    assert.deepEqual(p.sent, [])
    assert.equal(p.ui(), null)
    assert.ok(!p.root.classList.contains('__wis-popout-mode'))

    const empty = loadPopout({ videos: [] })
    empty.enter()
    assert.deepEqual(empty.sent, [])
  })
})

describe('the hover-clicked video', () => {
  /** Two videos: a small paused one at the top, a large playing one below. */
  function twoVideos() {
    const p = loadPopout({
      videos: [
        { left: 0, top: 0, width: 400, height: 225 },
        { playing: true, left: 0, top: 300, width: 1280, height: 720 },
      ],
    })
    p.mouse(200, 110)
    p.hoverButton().fire('click')
    return p
  }

  test('is pinned over a larger playing one', () => {
    const p = twoVideos()
    p.enter()
    assert.deepEqual(p.videos.map(pinned), [true, false])
  })

  test('is ignored six seconds after the click', () => {
    const p = twoVideos()
    p.clock.advance(5999)
    p.enter()
    assert.deepEqual(p.videos.map(pinned), [true, false], 'still within the window')

    const late = twoVideos()
    late.clock.advance(6000)
    late.enter()
    assert.deepEqual(late.videos.map(pinned), [false, true])
  })

  test('is used for one popout only', () => {
    const p = twoVideos()
    p.enter()
    p.exit()
    p.enter()
    assert.deepEqual(p.videos.map(pinned), [false, true])
  })

  test('is passed over once it left the page or lost its metadata', () => {
    const removed = twoVideos()
    removed.videos[0].parentNode.remove()
    removed.enter()
    assert.ok(pinned(removed.videos[1]))

    const emptied = twoVideos()
    emptied.videos[0].videoWidth = 0
    emptied.enter()
    assert.deepEqual(emptied.videos.map(pinned), [false, true])
  })
})
