import { beforeEach, describe, expect, it, vi } from 'vitest'

const ui = vi.hoisted(() => ({ locale: 'en' }))
vi.mock('./current', () => ({
  get locale() {
    return ui.locale
  },
}))

const {
  compareText,
  formatDate,
  formatDaysAgo,
  formatNumber,
  formatPercent,
  languageName,
  plural,
} = await import('./format')

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
    expect(plural(1_000_000, { one: 'one', other: 'other' })).toBe('other')
  })
  it('percentages and decimals', () => {
    expect(formatPercent(0.456)).toBe('46%')
    expect(formatNumber(1.5, { minimumFractionDigits: 1 })).toBe('1.5')
    ui.locale = 'es'
    // Intl puts a no-break space before the sign; which one varies by engine.
    expect(formatPercent(0.456)).toMatch(/^46\s%$/)
    expect(formatNumber(1.5, { minimumFractionDigits: 1 })).toBe('1,5')
  })
  it('English is British: day before month, 24-hour time', () => {
    const d = new Date(2026, 9, 7, 21, 40).getTime()
    expect(formatDate(d, { day: 'numeric', month: 'short' })).toBe('7 Oct')
    expect(formatDate(d, { hour: '2-digit', minute: '2-digit' })).toBe('21:40')
    expect(formatDaysAgo(0)).toBe('today')
    expect(formatDaysAgo(1)).toBe('yesterday')
    expect(formatDaysAgo(3)).toBe('3 days ago')
  })
  it('dates in other languages', () => {
    const d = new Date(2026, 9, 7).getTime()
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
