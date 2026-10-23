// The per-site rules the site panel gathers from settings (`features/site/lib/siteRules.ts`).

import assert from 'node:assert/strict'
import { describe, test } from 'node:test'

import { forgetSiteRule, siteRules } from '../../src/features/site/lib/siteRules.ts'

const settings = {
  media_permissions: {
    'camera|https://www.news.example': true,
    'microphone|https://news.example:8443': false,
    'camera|https://other.example': true,
  },
  autoplay_site_overrides: { 'news.example': 'allow', 'other.example': 'block' },
  zoom_site_overrides: { 'news.example': 125 },
  ua_site_overrides: { 'news.example': 'chrome', 'other.example': 'safari' },
  app_launch_permissions: { 'zoommtg|www.news.example': true, 'slack|other.example': false },
  user_cosmetic_rules: { 'www.news.example': ['.ad', '#promo'], 'other.example': ['.x'] },
}

describe('siteRules', () => {
  test('gathers every rule saved for the site, and only those', () => {
    assert.deepEqual(siteRules(settings, 'news.example'), [
      { id: 'media|camera|https://www.news.example', label: 'Camera', value: 'Allowed' },
      { id: 'media|microphone|https://news.example:8443', label: 'Microphone', value: 'Blocked' },
      { id: 'autoplay', label: 'Autoplay', value: 'Allow' },
      { id: 'zoom', label: 'Zoom', value: '125%' },
      { id: 'ua', label: 'User agent', value: 'Chrome' },
      { id: 'app_launch|zoommtg|www.news.example', label: 'zoommtg links', value: 'Allowed' },
      { id: 'hidden', label: 'Hidden elements', value: '2' },
    ])
  })

  test('is empty for a site with no rules', () => {
    assert.deepEqual(siteRules(settings, 'quiet.example'), [])
  })
})

describe('forgetSiteRule', () => {
  test('drops one permission and keeps the rest of the map', () => {
    assert.deepEqual(forgetSiteRule(settings, 'news.example', 'media|camera|https://www.news.example'), {
      media_permissions: {
        'microphone|https://news.example:8443': false,
        'camera|https://other.example': true,
      },
    })
    assert.deepEqual(forgetSiteRule(settings, 'news.example', 'app_launch|zoommtg|www.news.example'), {
      app_launch_permissions: { 'slack|other.example': false },
    })
  })

  test('drops the site from a per-host map', () => {
    assert.deepEqual(forgetSiteRule(settings, 'news.example', 'autoplay'), {
      autoplay_site_overrides: { 'other.example': 'block' },
    })
    assert.deepEqual(forgetSiteRule(settings, 'news.example', 'zoom'), { zoom_site_overrides: {} })
    assert.deepEqual(forgetSiteRule(settings, 'news.example', 'ua'), {
      ua_site_overrides: { 'other.example': 'safari' },
    })
    assert.deepEqual(forgetSiteRule(settings, 'news.example', 'hidden'), {
      user_cosmetic_rules: { 'other.example': ['.x'] },
    })
  })

  test('leaves settings alone for an unknown rule', () => {
    assert.deepEqual(forgetSiteRule(settings, 'news.example', 'nope'), {})
  })
})
