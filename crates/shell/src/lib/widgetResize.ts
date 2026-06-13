// Events between the widget-panel chrome overlays (`overlay-widgetresize`,
// `overlay-widgetheader`) and the main chrome. Payload of move/end is the
// horizontal drag delta (px) from the drag start.

export const WRESIZE_START_EVENT = 'wis-wresize-start'
export const WRESIZE_MOVE_EVENT = 'wis-wresize-move'
export const WRESIZE_END_EVENT = 'wis-wresize-end'
export const WPANEL_RELOAD_EVENT = 'wis-wpanel-reload'
/** Header mute button clicked; the chrome toggles the visible widget. */
export const WPANEL_MUTE_EVENT = 'wis-wpanel-mute'
/** Chrome -> header surface: the visible widget's state. */
export const WPANEL_STATE_EVENT = 'wis-wpanel-state'

export interface WPanelStatePayload {
  muted: boolean
  /** The visible panel's current URL; empty until its first navigation. */
  url: string
}

/** Panel webview -> chrome: a panel's committed URL on navigation. */
export const WPANEL_NAV_EVENT = 'wis-wpanel-nav'

export interface WPanelNavPayload {
  id: string
  url: string
}
