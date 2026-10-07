// Read a message catalogue (src/lib/strings/<locale>.ts) without running it: every
// entry's key, its English or translated text, a message's parameters, and the
// comments around it as context (docs/i18n/README.md). Uses TypeScript's parser.

import { readFileSync } from 'node:fs'
import ts from 'typescript'

/**
 * @typedef {object} Entry
 * @property {string} key       dotted path, e.g. `library.count`
 * @property {'text' | 'message'} kind  a plain string, or a function of values
 * @property {string[]} params  a message's parameters, e.g. `n: number`
 * @property {string} text      the string, or a message's body as written
 * @property {string[]} context comments that apply: the group's, then the entry's
 */

/** @returns {Entry[]} */
export function readCatalogue(path) {
  const source = readFileSync(path, 'utf8')
  const file = ts.createSourceFile(path, source, ts.ScriptTarget.Latest, true)
  let root
  file.forEachChild((node) => {
    if (!ts.isVariableStatement(node)) return
    for (const d of node.declarationList.declarations) {
      let init = d.initializer
      if (init && ts.isAsExpression(init)) init = init.expression
      if (init && ts.isObjectLiteralExpression(init) && !root) root = init
    }
  })
  if (!root) throw new Error(`${path}: no catalogue object`)

  const comments = (node) =>
    (ts.getLeadingCommentRanges(source, node.getFullStart()) ?? []).map((r) => ({
      doc: source.startsWith('/**', r.pos),
      text: source
        .slice(r.pos, r.end)
        .replace(/^\/\*\*?|\*\/$/g, '')
        .replace(/^\s*\*\s?/gm, '')
        .replace(/^\/\/\s?/gm, '')
        .replace(/\s+/g, ' ')
        .trim(),
    }))

  /** @type {Entry[]} */
  const out = []
  const walk = (object, path, inherited) => {
    // A `//` comment heads the entries after it, up to the next one; `/** */`
    // belongs to its entry alone.
    let group = []
    for (const p of object.properties) {
      if (!ts.isPropertyAssignment(p)) continue
      const own = comments(p)
      const key = [...path, p.name.getText(file).replace(/^['"]|['"]$/g, '')]
      const v = p.initializer
      // A section's comment covers that section only.
      if (ts.isObjectLiteralExpression(v)) {
        walk(v, key, [...inherited, ...own.map((c) => c.text)])
        group = []
        continue
      }
      const line = own.filter((c) => !c.doc).map((c) => c.text)
      if (line.length) group = line
      const notes = [...inherited, ...group, ...own.filter((c) => c.doc).map((c) => c.text)]
      if (ts.isStringLiteral(v) || ts.isNoSubstitutionTemplateLiteral(v))
        out.push({ key: key.join('.'), kind: 'text', params: [], text: v.text, context: notes })
      else if (ts.isArrowFunction(v))
        out.push({
          key: key.join('.'),
          kind: 'message',
          params: v.parameters.map((x) => x.getText(file)),
          text: v.body.getText(file).replace(/\s+/g, ' '),
          context: notes,
        })
      else throw new Error(`${path}: ${key.join('.')} is neither text nor a message`)
    }
  }
  walk(root, [], [])
  return out
}

/** Words a translator reads: the text without code (`${…}`, `plural(…)`). */
export function words(entry) {
  const text =
    entry.kind === 'text'
      ? entry.text
      : [...entry.text.matchAll(/['`]((?:[^'`\\]|\\.)*)['`]/g)].map((m) => m[1]).join(' ')
  return text
    .replace(/\$\{[^}]*\}/g, ' ')
    .split(/\s+/)
    .filter((w) => /\p{L}/u.test(w)).length
}
