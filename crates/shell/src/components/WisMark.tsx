/**
 * The Wis logo:
 *   - canvas 200x200, line weight 9
 *   - pupils r=15, eyes blink (r 15->0->15) for state
 *   - currentColor everywhere so callers control colour via CSS
 *   - one mark, three meanings: open (rest), closed (private/reader),
 *     blink/loading.
 *
 * Variants:
 *   - 'solid-open' / 'solid-closed' / 'line-open' / 'line-closed': static
 *   - 'blink': natural ~3.4s blink, for ambient/at-rest brand surfaces
 *   - 'loading': 1.4s rapid blink, replaces the tab spinner
 */

export type WisMarkVariant =
  | 'solid-open'
  | 'solid-closed'
  | 'line-open'
  | 'line-closed'
  | 'blink'
  | 'loading'

interface WisMarkProps {
  variant?: WisMarkVariant
  size?: number | string
  className?: string
  'aria-label'?: string
}

const HEAD_PATH =
  'M32 72 L58 34 L82 72 L100 58 L118 72 L142 34 L168 72 C168 128 144 168 100 168 C56 168 32 128 32 72 Z'
const BEAK_PATH = 'M100 122 L106 132 L100 138 L94 132 Z'
const LEFT_LID = 'M62 108 Q76 100 90 108'
const RIGHT_LID = 'M110 108 Q124 100 138 108'

export function WisMark({
  variant = 'solid-open',
  size = 32,
  className,
  'aria-label': ariaLabel = 'wis',
}: WisMarkProps) {
  const common = {
    xmlns: 'http://www.w3.org/2000/svg',
    viewBox: '0 0 200 200',
    width: size,
    height: size,
    role: 'img' as const,
    'aria-label': ariaLabel,
    className,
  }

  if (variant === 'line-open' || variant === 'line-closed') {
    return (
      <svg
        {...common}
        fill="none"
        stroke="currentColor"
        strokeWidth={9}
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <path d={HEAD_PATH} />
        {variant === 'line-open' ? (
          <>
            <circle cx={76} cy={108} r={15} fill="currentColor" stroke="none" />
            <circle cx={124} cy={108} r={15} fill="currentColor" stroke="none" />
          </>
        ) : (
          <>
            <path d={LEFT_LID} />
            <path d={RIGHT_LID} />
          </>
        )}
        <path d={BEAK_PATH} fill="currentColor" stroke="none" />
      </svg>
    )
  }

  // Solid / animated variants share a mask-based render.
  const maskId = `wm-${variant}`
  const isBlink = variant === 'blink'
  const isLoading = variant === 'loading'
  const isAnimated = isBlink || isLoading
  const closed = variant === 'solid-closed'

  // Blink keyframes:
  //   natural: 3.4s, closed pose held .86->.94 of timeline (≈140ms)
  //   loading: 1.4s, 50% duty cycle (long closed pose for a visible blink)
  const dur = isBlink ? '3.4s' : isLoading ? '1.4s' : undefined
  const pupilValues = isBlink
    ? '15;15;0;15;15'
    : isLoading
      ? '15;0;15'
      : undefined
  const pupilKeyTimes = isBlink
    ? '0;0.86;0.9;0.94;1'
    : isLoading
      ? '0;0.5;1'
      : undefined
  const lidValues = isBlink
    ? '0;0;1;0;0'
    : isLoading
      ? '0;1;0'
      : undefined
  const lidKeyTimes = pupilKeyTimes

  // Staggered open <-> closed transitions for Brand and the launchpad hero
  const ease = 'cubic-bezier(.4,0,.2,1)'
  const pupilStyle = isAnimated
    ? undefined
    : closed
      ? { transition: `r 150ms ${ease} 0ms` }
      : { transition: `r 150ms ${ease} 110ms` }
  const lidStyle = isAnimated
    ? undefined
    : closed
      ? { transition: `opacity 110ms ${ease} 120ms` }
      : { transition: `opacity 110ms ${ease} 0ms` }

  return (
    <svg {...common} fill="currentColor">
      <defs>
        <mask id={maskId} maskUnits="userSpaceOnUse">
          <rect width={200} height={200} fill="#fff" />
          {/* Pupils: animate r for blink/loading */}
          <circle cx={76} cy={108} r={closed ? 0 : 15} fill="#000" style={pupilStyle}>
            {isAnimated && (
              <animate
                attributeName="r"
                values={pupilValues}
                keyTimes={pupilKeyTimes}
                dur={dur}
                repeatCount="indefinite"
              />
            )}
          </circle>
          <circle cx={124} cy={108} r={closed ? 0 : 15} fill="#000" style={pupilStyle}>
            {isAnimated && (
              <animate
                attributeName="r"
                values={pupilValues}
                keyTimes={pupilKeyTimes}
                dur={dur}
                repeatCount="indefinite"
              />
            )}
          </circle>
          {/* Closed lids: shown statically when closed, faded in for blinks */}
          <g opacity={closed ? 1 : 0} style={lidStyle}>
            <path
              d={LEFT_LID}
              stroke="#000"
              strokeWidth={6}
              fill="none"
              strokeLinecap="round"
            >
              {isAnimated && (
                <animate
                  attributeName="opacity"
                  values={lidValues}
                  keyTimes={lidKeyTimes}
                  dur={dur}
                  repeatCount="indefinite"
                />
              )}
            </path>
            <path
              d={RIGHT_LID}
              stroke="#000"
              strokeWidth={6}
              fill="none"
              strokeLinecap="round"
            >
              {isAnimated && (
                <animate
                  attributeName="opacity"
                  values={lidValues}
                  keyTimes={lidKeyTimes}
                  dur={dur}
                  repeatCount="indefinite"
                />
              )}
            </path>
          </g>
          <path d={BEAK_PATH} fill="#000" />
        </mask>
      </defs>
      <path d={HEAD_PATH} mask={`url(#${maskId})`} />
    </svg>
  )
}
