export const transformOf = (x: number, y: number): string => `translate3d(${x}px,${y}px,0)`

export function follow(move: (e: PointerEvent) => void, done: (e: PointerEvent) => void): void {
  const up = (e: PointerEvent) => {
    window.removeEventListener('pointermove', move)
    window.removeEventListener('pointerup', up)
    done(e)
  }
  window.addEventListener('pointermove', move)
  window.addEventListener('pointerup', up)
}
