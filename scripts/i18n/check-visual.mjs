#!/usr/bin/env node
// Judge the visual passes in i18n-out/visual/ (Stage 6, plan §6): for each
// language, every screen was captured, no UI text overflows where English does
// not, nothing is left in English, and the catalogue lacks no key. Exit 1 if not.
//
//   node scripts/i18n/check-visual.mjs [tag…]    (default: every language run)
//
// The runs come from scripts/i18n/check-languages.sh.

import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

const ROOT = new URL('../..', import.meta.url).pathname
const DIR = join(ROOT, 'i18n-out/visual')

function report(tag) {
  const dir = join(DIR, tag)
  const file = existsSync(dir) && readdirSync(dir).find((f) => f.endsWith('.json'))
  return file ? JSON.parse(readFileSync(join(dir, file), 'utf8')) : null
}

/** An overflowing element, by screen and element (its text differs by language). */
const where = (capture, item) => `${capture} ${item.split(':')[0]}`

const english = report('en')
if (!english) {
  console.error('No English run in i18n-out/visual/en: run scripts/i18n/check-languages.sh')
  process.exit(1)
}
const screens = Object.keys(english.raw.shots)
const englishOverflow = new Set(
  Object.entries(english.raw.overflow ?? {}).flatMap(([c, items]) => items.map((i) => where(c, i))),
)

const tags = process.argv.slice(2).length
  ? process.argv.slice(2)
  : readdirSync(DIR).filter((d) => d !== 'en' && report(d))
let failed = 0
for (const tag of tags) {
  const r = report(tag)
  const problems = []
  if (!r) problems.push('no run')
  else {
    const missing = screens.filter((s) => !(s in r.raw.shots))
    if (missing.length) problems.push(`screens not captured: ${missing.join(', ')}`)
    for (const [capture, items] of Object.entries(r.raw.overflow ?? {}))
      for (const item of items)
        if (!englishOverflow.has(where(capture, item)))
          problems.push(`overflow on ${capture}: ${item}`)
    for (const [capture, items] of Object.entries(r.raw.english ?? {}))
      for (const item of items) problems.push(`English on ${capture}: ${item}`)
    if (r.raw.missingKeys?.length) problems.push(`missing keys: ${r.raw.missingKeys.join(', ')}`)
  }
  console.log(`${problems.length ? 'FAIL' : 'PASS'} ${tag}`)
  for (const p of problems) console.log(`  ${p}`)
  if (problems.length) failed++
}
process.exit(failed ? 1 : 0)
