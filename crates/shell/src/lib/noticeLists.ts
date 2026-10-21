import type { UserSettingsPatch } from '@/bindings/types'

export type NoticeList = 'cookie_notices' | 'annoyances'

/** The annoyance list contains the cookie list, so one switch carries the other. */
export function noticeListsPatch(list: NoticeList, on: boolean): UserSettingsPatch {
  if (list === 'annoyances') {
    return on ? { block_annoyances: true, block_cookie_notices: true } : { block_annoyances: false }
  }
  return on ? { block_cookie_notices: true } : { block_cookie_notices: false, block_annoyances: false }
}
