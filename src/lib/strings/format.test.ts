import { beforeEach, describe, expect, it, vi } from 'vitest'

const ui = vi.hoisted(() => ({ locale: 'en' }))
vi.mock('./index', () => ({
  get locale() {
    return ui.locale
  },
}))

const { compareText, formatBytes, formatDate, formatDaysAgo, formatPercent, languageName, plural } =
  await import('./format')

const forms = { one: 'one', many: 'many', other: 'other' }

describe('L-1 formatting in the UI language', () => {
  beforeEach(() => {
    ui.locale = 'en'
  })
  it('plurals follow each language', () => {
    expect(plural(1, forms)).toBe('one')
    expect(plural(2, forms)).toBe('other')
    ui.locale = 'ja'
    expect(plural(1, forms)).toBe('other')
    ui.locale = 'zh-Hant'
    expect(plural(1, forms)).toBe('other')
    ui.locale = 'es'
    expect(plural(1, forms)).toBe('one')
    expect(plural(3, forms)).toBe('other')
    // A message without `many` falls back to `other`.
    expect(plural(1_000_000, { one: 'one', other: 'other' })).toMatch(/^(other|many)$/)
  })
  it('percentages and sizes', () => {
    expect(formatPercent(0.456)).toBe('46%')
    expect(formatBytes(1.5 * (1 << 20))).toBe('1.5 MB')
    expect(formatBytes(300)).toBe('1 kB')
    ui.locale = 'es'
    // Intl puts a no-break space before the unit; which one varies by engine.
    expect(formatPercent(0.456)).toMatch(/^46\s%$/)
    expect(formatBytes(1.5 * (1 << 20))).toMatch(/^1,5\sMB$/)
  })
  it('dates and days ago', () => {
    const d = new Date(2026, 9, 7).getTime()
    expect(formatDate(d, { day: 'numeric', month: 'short' })).toBe('Oct 7')
    expect(formatDaysAgo(0)).toBe('today')
    expect(formatDaysAgo(1)).toBe('yesterday')
    expect(formatDaysAgo(3)).toBe('3 days ago')
    ui.locale = 'ja'
    expect(formatDate(d, { day: 'numeric', month: 'short' })).toBe('10月7日')
    expect(formatDaysAgo(1)).toBe('昨日')
    ui.locale = 'es'
    expect(formatDaysAgo(1)).toBe('ayer')
  })
  it('language names and sorting', () => {
    expect(languageName('ja')).toBe('Japanese')
    expect(languageName('not a tag!')).toBe('not a tag!')
    ui.locale = 'zh-Hans'
    expect(languageName('ja')).toBe('日语')
    ui.locale = 'en'
    expect(['Book 10', 'book 2', 'Émile'].sort(compareText)).toEqual(['book 2', 'Book 10', 'Émile'])
  })
  it('the pseudo-locale formats as English', () => {
    ui.locale = 'en-XA'
    expect(formatPercent(0.5)).toBe('50%')
  })
})
