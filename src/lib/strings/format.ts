// Plurals, numbers, dates and sorting in the UI language (L-1, L-7; docs/i18n-plan.md §4).
// Messages call these instead of English rules (`n === 1 ? …`) or fixed locales.

import { locale } from './index'

/** The tag Intl gets: the pseudo-locale formats as English. */
const intlLocale = () => (locale === 'en-XA' ? 'en' : locale)

const cache = new Map<string, unknown>()
function cached<T>(kind: string, options: object, make: (l: string) => T): T {
  const key = `${kind}|${intlLocale()}|${JSON.stringify(options)}`
  let v = cache.get(key) as T | undefined
  if (!v) cache.set(key, (v = make(intlLocale())))
  return v
}

export type PluralForms = Partial<Record<Intl.LDMLPluralRule, string>> & { other: string }

/** The form of `forms` for `n` (Chinese and Japanese have only `other`; Spanish `many` falls back). */
export function plural(n: number, forms: PluralForms): string {
  const rule = cached('plural', {}, (l) => new Intl.PluralRules(l)).select(n)
  return forms[rule] ?? forms.other
}

export const formatNumber = (n: number, options: Intl.NumberFormatOptions = {}) =>
  cached('number', options, (l) => new Intl.NumberFormat(l, options)).format(n)

/** A fraction as a whole percentage: 0.45 → “45%” (en), “45 %” (es). */
export const formatPercent = (fraction: number) =>
  formatNumber(fraction, { style: 'percent', maximumFractionDigits: 0 })

/** A file size in KB or MB, one decimal for MB (“1.5 MB”, “1,5 MB”). */
export function formatBytes(bytes: number): string {
  return bytes >= 1 << 20
    ? formatNumber(bytes / (1 << 20), {
        style: 'unit',
        unit: 'megabyte',
        minimumFractionDigits: 1,
        maximumFractionDigits: 1,
      })
    : formatNumber(Math.max(1, Math.round(bytes / 1024)), { style: 'unit', unit: 'kilobyte' })
}

export const formatDate = (ms: number, options: Intl.DateTimeFormatOptions) =>
  cached('date', options, (l) => new Intl.DateTimeFormat(l, options)).format(ms)

/** Whole days relative to today: 0 → “today”, -1 → “yesterday”, -3 → “3 days ago”. */
export const formatDaysAgo = (days: number) =>
  cached('relative', {}, (l) => new Intl.RelativeTimeFormat(l, { numeric: 'auto' })).format(
    -days,
    'day',
  )

/** A book's language named in the UI language (“Japanese”, “日语”). */
export function languageName(tag: string): string {
  try {
    return (
      cached('language', {}, (l) => new Intl.DisplayNames([l], { type: 'language' })).of(tag) ?? tag
    )
  } catch {
    return tag
  }
}

/** L-7: library order in the UI language, numbers by value, case and accents ignored. */
export const compareText = (a: string, b: string) =>
  cached(
    'collator',
    {},
    (l) => new Intl.Collator(l, { numeric: true, sensitivity: 'base' }),
  ).compare(a, b)
