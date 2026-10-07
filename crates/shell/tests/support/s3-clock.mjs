// A virtual clock for page scripts: timers, intervals and animation frames run only when a test advances them.

export function createClock(start = 1_000_000) {
  let now = start
  let nextId = 1
  const timers = new Map()
  const frames = new Map()

  const schedule = (fn, ms, every) => {
    const id = nextId++
    timers.set(id, { at: now + Math.max(0, Number(ms) || 0), fn, every })
    return id
  }
  const cancel = (id) => void timers.delete(id)

  return {
    Date: { now: () => now },
    setTimeout: (fn, ms) => schedule(fn, ms, 0),
    clearTimeout: cancel,
    setInterval: (fn, ms) => schedule(fn, ms, Math.max(1, Number(ms) || 0)),
    clearInterval: cancel,
    requestAnimationFrame: (fn) => {
      const id = nextId++
      frames.set(id, fn)
      return id
    },
    cancelAnimationFrame: (id) => void frames.delete(id),

    /** Run every timer due within the next `ms`, in time order. */
    advance(ms) {
      const end = now + ms
      for (;;) {
        let next = null
        for (const [id, timer] of timers) {
          if (timer.at <= end && (!next || timer.at < next.timer.at)) next = { id, timer }
        }
        if (!next) break
        now = next.timer.at
        if (next.timer.every) next.timer.at += next.timer.every
        else timers.delete(next.id)
        next.timer.fn()
      }
      now = end
    },

    /** Run the animation frames requested so far. */
    frame() {
      const due = [...frames.values()]
      frames.clear()
      due.forEach((fn) => fn(now))
    },

    pendingFrames: () => frames.size,
  }
}
