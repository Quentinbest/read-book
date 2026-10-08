// The pseudo-locale `en-XA` (development builds only): accented, about 35 % longer
// and bracketed, so overflow and strings that bypass the catalogue stand out.
// Values passed into a message (titles, numbers) are left as they are.

import type { Messages } from './types'

const ACCENTS: Record<string, string> = {
  a: 'á',
  e: 'é',
  i: 'í',
  o: 'ö',
  u: 'ü',
  c: 'ç',
  n: 'ñ',
  y: 'ý',
  s: 'š',
  z: 'ž',
  A: 'Å',
  E: 'É',
  I: 'Í',
  O: 'Ö',
  U: 'Ü',
  C: 'Ç',
  N: 'Ñ',
  Y: 'Ý',
  S: 'Š',
  Z: 'Ž',
}

const accent = (s: string) => s.replace(/[a-zA-Z]/g, (c) => ACCENTS[c] ?? c)
const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')

/** Accents `s` except where it contains one of `keep`, then pads and brackets it. */
export function pseudo(s: string, keep: readonly string[] = []): string {
  const parts = keep.filter(Boolean).sort((a, b) => b.length - a.length)
  const body = parts.length
    ? s
        .split(new RegExp(`(${parts.map(escape).join('|')})`))
        .map((p, i) => (i % 2 ? p : accent(p)))
        .join('')
    : accent(s)
  return `⟦${body}${'~'.repeat(Math.ceil(s.length * 0.35))}⟧`
}

function pseudoValue(v: unknown): unknown {
  if (typeof v === 'string') return pseudo(v)
  if (typeof v === 'function') {
    return (...args: unknown[]) =>
      pseudo(
        (v as (...a: unknown[]) => string)(...args),
        args.flatMap((a) => (typeof a === 'string' || typeof a === 'number' ? [String(a)] : [])),
      )
  }
  if (v && typeof v === 'object')
    return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, pseudoValue(x)]))
  return v
}

export const pseudoLocalize = (m: Messages): Messages => pseudoValue(m) as Messages
