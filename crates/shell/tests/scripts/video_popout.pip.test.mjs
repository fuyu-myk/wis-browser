// The video popout script's native Picture-in-Picture path, used on Windows where the shell sets `__wisPopoutPip`.

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { loadPopout } from '../support/s3-popoutPage.mjs'

function pipPage(options = {}) {
  return loadPopout({ pip: true, ...options })
}

describe('with the PiP flag', () => {
  test('the custom popout never enters', () => {
    const p = pipPage({ videos: [{ playing: true }] })
    p.enter()
    assert.deepEqual(p.sent, [])
    assert.equal(p.ui(), null)
  })

  test('media reports arm automatic PiP on the playing video once, instead of posting', () => {
    const p = pipPage({ videos: [{ playing: true }] })
    const [v] = p.videos
    p.media(v, 'play')
    p.settle()
    assert.deepEqual(p.sent, [])
    assert.equal(v.autoPictureInPicture, true)
    assert.equal(p.actions.enterpictureinpicture.length, 1)
    p.media(v, 'volumechange')
    p.settle()
    assert.equal(p.actions.enterpictureinpicture.length, 1)
  })

  test('paused videos are not armed', () => {
    const p = pipPage({ videos: [{}] })
    p.media(p.videos[0], 'pause')
    p.settle()
    assert.equal(p.videos[0].autoPictureInPicture, undefined)
  })

  test('the media session action enters PiP on the armed video', () => {
    const p = pipPage({ videos: [{ playing: true }] })
    p.media(p.videos[0], 'play')
    p.settle()
    p.actions.enterpictureinpicture[0]()
    assert.equal(p.videos[0].pipRequests, 1)
  })

  test('arming works on a page without mediaSession', () => {
    const p = pipPage({ videos: [{ playing: true }], mediaSession: false })
    p.media(p.videos[0], 'play')
    p.settle()
    assert.equal(p.videos[0].autoPictureInPicture, true)
  })

  test('the hover button requests PiP on its own video directly, without asking native', () => {
    const p = pipPage({
      videos: [
        { left: 0, top: 0, width: 400, height: 225 },
        { playing: true, left: 0, top: 300, width: 1280, height: 720 },
      ],
    })
    p.mouse(200, 110)
    p.hoverButton().fire('click')
    assert.deepEqual(
      p.videos.map((v) => v.pipRequests),
      [1, 0],
    )
    assert.deepEqual(p.sent, [])
  })
})

describe("the shell's enter and exit PiP", () => {
  test('enter requests PiP on the playing video over a larger paused one', () => {
    const p = pipPage({ videos: [{ width: 1280, height: 720 }, { playing: true }] })
    p.enterPip()
    assert.deepEqual(
      p.videos.map((v) => v.pipRequests),
      [0, 1],
    )
  })

  test('with nothing playing, enter goes to the last video seen playing, else the largest', () => {
    const p = pipPage({ videos: [{ playing: true }, { width: 800, height: 450 }] })
    const [last] = p.videos
    p.media(last, 'play')
    p.settle()
    last.paused = true
    last.rect = { left: 0, top: 0, width: 100, height: 50 }
    p.enterPip()
    assert.deepEqual(
      p.videos.map((v) => v.pipRequests),
      [1, 0],
    )

    const q = pipPage({ videos: [{ width: 300, height: 200 }, { width: 800, height: 450 }] })
    q.enterPip()
    assert.deepEqual(
      q.videos.map((v) => v.pipRequests),
      [0, 1],
    )
  })

  test('enter does nothing without a usable video', () => {
    const p = pipPage({ videos: [{ width: 100, height: 50 }] })
    p.enterPip()
    assert.equal(p.videos[0].pipRequests, 0)
  })

  test('a video that disables PiP, or is already in PiP, is not requested again', () => {
    const blocked = pipPage({ videos: [{ playing: true }] })
    blocked.videos[0].disablePictureInPicture = true
    blocked.enterPip()
    assert.equal(blocked.videos[0].pipRequests, 0)

    const already = pipPage({ videos: [{ playing: true }] })
    already.document.pictureInPictureElement = already.videos[0]
    already.enterPip()
    assert.equal(already.videos[0].pipRequests, 0)
  })

  test('a refused or throwing PiP request is swallowed', () => {
    const refused = pipPage({ videos: [{ playing: true }] })
    refused.page.pipRejects = true
    refused.enterPip()
    assert.equal(refused.videos[0].pipRequests, 1)

    const throwing = pipPage({ videos: [{ playing: true }] })
    throwing.page.pipThrows = true
    assert.doesNotThrow(() => throwing.enterPip())
  })

  test('exit leaves the PiP window only when one is open', () => {
    const p = pipPage({ videos: [{ playing: true }] })
    p.exitPip()
    assert.equal(p.document.pipExits, 0)
    p.document.pictureInPictureElement = p.videos[0]
    p.exitPip()
    assert.equal(p.document.pipExits, 1)
  })
})
