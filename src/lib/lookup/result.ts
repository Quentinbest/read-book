// LK2 (Reading Lens): what a lookup provider returns, in fixed fields of plain
// text that the core renders. Anything else is refused: unknown fields, a label of
// the provider's own, HTML, percentages or confidence scores (EQ1, EQ2). The
// label above an answer comes from a closed set the core owns.

export type LookupStatus = 'ok' | 'needs_context' | 'error'

/** LK10: failures a provider reports; the core words them and offers the action. */
export type LookupErrorKind = 'offline' | 'unauthorized' | 'rate_limited' | 'unavailable'

export interface LookupResult {
  status: LookupStatus
  headword: string
  /** A normalised form of the selection (“invalidate” for “invalidates”). */
  term?: string
  meaning: string
  /** EQ3: a qualifier that changes the meaning; shown in the first layer, never only under More. */
  qualifier?: string
  /** The More layer. */
  details?: { label: string; text: string }[]
  source: { kind: 'ai' | 'dictionary'; name: string; model?: string }
  /** EX6: what was sent, as the provider sent it. */
  sent?: string
  /** EQ4, needs_context: what is missing. */
  missing?: string
  /** LK10, error: what went wrong. */
  error?: LookupErrorKind
}

export const LIMITS = {
  headword: 200,
  term: 200,
  meaning: 2000,
  qualifier: 300,
  detailLabel: 60,
  detailText: 2000,
  details: 8,
  name: 60,
  model: 80,
  sent: 6000,
  missing: 300,
} as const

const TOP = new Set([
  'status',
  'headword',
  'term',
  'meaning',
  'qualifier',
  'details',
  'source',
  'sent',
  'missing',
  'error',
])
const SOURCE = new Set(['kind', 'name', 'model'])
const ERRORS = new Set<LookupErrorKind>(['offline', 'unauthorized', 'rate_limited', 'unavailable'])
/** EQ2: no percentages and no confidence or probability talk. */
const SCORE = /\d\s*%|\bpercent\b|\bconfiden(?:ce|t)\b|\bprobabilit/i
/** Plain text only: markup is never rendered, so it is refused rather than shown as tags. */
const MARKUP = /<\/?[a-z!][^>]*>/i

export type Validated = { ok: true; result: LookupResult } | { ok: false; why: string }

function text(
  v: unknown,
  field: string,
  max: number,
  required: boolean,
  scored = true,
): string | undefined {
  if (v === undefined || v === null) {
    if (required) throw new Error(`${field} is missing`)
    return undefined
  }
  if (typeof v !== 'string') throw new Error(`${field} is not text`)
  if (v.length > max) throw new Error(`${field} is longer than ${max} characters`)
  if (MARKUP.test(v)) throw new Error(`${field} holds markup`)
  if (scored && SCORE.test(v)) throw new Error(`${field} holds a percentage or a score`)
  return v
}

/** LK2: check a provider's answer; the reason is for the extension's author (and tests). */
export function validateResult(raw: unknown): Validated {
  try {
    if (!raw || typeof raw !== 'object' || Array.isArray(raw)) throw new Error('not an object')
    const r = raw as Record<string, unknown>
    for (const k of Object.keys(r)) if (!TOP.has(k)) throw new Error(`unknown field “${k}”`)
    const status = r.status
    if (status !== 'ok' && status !== 'needs_context' && status !== 'error')
      throw new Error('status must be ok, needs_context or error')
    const s = r.source as Record<string, unknown> | undefined
    if (!s || typeof s !== 'object' || Array.isArray(s)) throw new Error('source is missing')
    for (const k of Object.keys(s))
      if (!SOURCE.has(k)) throw new Error(`source has an unknown field “${k}” (labels are Linen's)`)
    if (s.kind !== 'ai' && s.kind !== 'dictionary')
      throw new Error('source.kind must be ai or dictionary')
    const result: LookupResult = {
      status,
      headword: text(r.headword, 'headword', LIMITS.headword, true)!,
      meaning: text(r.meaning, 'meaning', LIMITS.meaning, status === 'ok') ?? '',
      source: {
        kind: s.kind,
        name: text(s.name, 'source.name', LIMITS.name, true)!,
      },
    }
    const model = text(s.model, 'source.model', LIMITS.model, false)
    if (model) result.source.model = model
    const term = text(r.term, 'term', LIMITS.term, false)
    if (term) result.term = term
    const qualifier = text(r.qualifier, 'qualifier', LIMITS.qualifier, false)
    if (qualifier) result.qualifier = qualifier
    // What was sent is the reader's own text: a percentage in the book is not a score.
    const sent = text(r.sent, 'sent', LIMITS.sent, false, false)
    if (sent) result.sent = sent
    if (r.details !== undefined) {
      if (!Array.isArray(r.details)) throw new Error('details is not a list')
      if (r.details.length > LIMITS.details) throw new Error(`more than ${LIMITS.details} details`)
      result.details = r.details.map((d, i) => {
        if (!d || typeof d !== 'object') throw new Error(`details[${i}] is not an object`)
        const o = d as Record<string, unknown>
        for (const k of Object.keys(o))
          if (k !== 'label' && k !== 'text')
            throw new Error(`details[${i}] has an unknown field “${k}”`)
        return {
          label: text(o.label, `details[${i}].label`, LIMITS.detailLabel, true)!,
          text: text(o.text, `details[${i}].text`, LIMITS.detailText, true)!,
        }
      })
    }
    if (status === 'needs_context')
      result.missing = text(r.missing, 'missing', LIMITS.missing, true)
    else if (r.missing !== undefined) throw new Error('missing belongs to needs_context')
    if (status === 'error') {
      if (!ERRORS.has(r.error as LookupErrorKind))
        throw new Error('error must be offline, unauthorized, rate_limited or unavailable')
      result.error = r.error as LookupErrorKind
    } else if (r.error !== undefined) throw new Error('error belongs to status error')
    return { ok: true, result }
  } catch (e) {
    return { ok: false, why: (e as Error).message }
  }
}
