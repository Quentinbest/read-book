// `when` clauses on selection actions (P1, P10): the closed language the core
// checks at install (src-tauri/src/extensions/when.rs), evaluated here.

export interface WhenContext {
  'selection.words': number
  'selection.chars': number
  'selection.language': string
  'book.language': string
  'book.fixedLayout': boolean
  /** LK9: the book's primary language subtag, lowercase; "" when it has none or `und`. */
  'book.lang': string
  /** LK9: sentences in the selection (Intl.Segmenter). */
  'selection.sentences': number
}

type Value = number | string | boolean
type Token =
  | { t: 'id'; v: keyof WhenContext }
  | { t: 'lit'; v: Value }
  | { t: 'op'; v: string }
  | { t: '&&' | '||' | '!' | '(' | ')' }

const VARIABLES = new Set<string>([
  'selection.words',
  'selection.chars',
  'selection.language',
  'book.language',
  'book.fixedLayout',
  'book.lang',
  'selection.sentences',
])

/** LK9 (prov., O12): `en-GB` → `en`, `EN-us` → `en`; a missing language or `und` → "". */
export function primaryLang(tag: string | null | undefined): string {
  const primary = (tag ?? '').trim().split(/[-_]/)[0].toLowerCase()
  return primary === 'und' ? '' : primary
}

function tokens(s: string): Token[] {
  const out: Token[] = []
  let i = 0
  while (i < s.length) {
    const c = s[i]
    if (c === ' ') {
      i++
    } else if (c === '(' || c === ')') {
      out.push({ t: c })
      i++
    } else if (s.startsWith('&&', i) || s.startsWith('||', i)) {
      out.push({ t: s.slice(i, i + 2) as '&&' | '||' })
      i += 2
    } else if ('<>=!'.includes(c)) {
      const two = s[i + 1] === '='
      if (c === '!' && !two) {
        out.push({ t: '!' })
        i++
      } else if (c === '=' && !two) {
        throw new Error('use == to compare')
      } else {
        out.push({ t: 'op', v: s.slice(i, i + (two ? 2 : 1)) })
        i += two ? 2 : 1
      }
    } else if (c === '"') {
      const end = s.indexOf('"', i + 1)
      if (end < 0) throw new Error('unclosed string')
      out.push({ t: 'lit', v: s.slice(i + 1, end) })
      i = end + 1
    } else if (/[0-9]/.test(c)) {
      const m = /^[0-9.]+/.exec(s.slice(i))![0]
      out.push({ t: 'lit', v: Number(m) })
      i += m.length
    } else if (/[a-zA-Z]/.test(c)) {
      const m = /^[a-zA-Z0-9.]+/.exec(s.slice(i))![0]
      if (m === 'true' || m === 'false') out.push({ t: 'lit', v: m === 'true' })
      else if (VARIABLES.has(m)) out.push({ t: 'id', v: m as keyof WhenContext })
      else throw new Error(`unknown variable “${m}”`)
      i += m.length
    } else {
      throw new Error(`unexpected “${c}”`)
    }
  }
  return out
}

/** Evaluate a clause; a clause that does not parse is false (the core refused it at install). */
export function when(clause: string | null | undefined, ctx: WhenContext): boolean {
  if (!clause) return true
  try {
    const t = tokens(clause)
    let p = 0
    const peek = () => t[p]
    const term = (): Value => {
      const k = t[p++]
      if (!k) throw new Error('expected a value')
      if (k.t === 'id') return ctx[k.v]
      if (k.t === 'lit') return k.v
      throw new Error('expected a value')
    }
    const compare = (a: Value, op: string, b: Value): boolean => {
      switch (op) {
        case '==':
          return a === b
        case '!=':
          return a !== b
        case '<':
          return a < b
        case '<=':
          return a <= b
        case '>':
          return a > b
        case '>=':
          return a >= b
      }
      throw new Error(op)
    }
    const unary = (): boolean => {
      const k = peek()
      if (k?.t === '!') {
        p++
        return !unary()
      }
      if (k?.t === '(') {
        p++
        const v = expr()
        if (t[p++]?.t !== ')') throw new Error('missing )')
        return v
      }
      const a = term()
      const o = peek()
      if (o?.t === 'op') {
        p++
        return compare(a, o.v, term())
      }
      return Boolean(a)
    }
    const and = (): boolean => {
      let v = unary()
      while (peek()?.t === '&&') {
        p++
        v = unary() && v
      }
      return v
    }
    const expr = (): boolean => {
      let v = and()
      while (peek()?.t === '||') {
        p++
        v = and() || v
      }
      return v
    }
    const v = expr()
    if (p !== t.length) throw new Error('trailing text')
    return v
  } catch {
    return false
  }
}
