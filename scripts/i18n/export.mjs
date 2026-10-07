#!/usr/bin/env node
// Export the catalogue for translators and reviewers (docs/i18n/README.md, L-6):
// one CSV per language, a row per entry, with its context, the English, the
// translation so far and the room it has. Opens in Numbers, Excel or Sheets.
//
//   node scripts/i18n/export.mjs ja          → i18n-out/ja.csv
//   node scripts/i18n/export.mjs             → every language in LOCALES
//
// Messages are shown as written in TypeScript (`${n} books`): `${…}` are values
// Linen fills in, and must stay.

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { readCatalogue, words } from './catalogue.mjs'

const ROOT = new URL('../..', import.meta.url).pathname
const STRINGS = join(ROOT, 'src/lib/strings')
const OUT = join(ROOT, 'i18n-out')
const LOCALES = ['zh-Hans', 'zh-Hant', 'ja', 'es']

const limits = JSON.parse(readFileSync(join(ROOT, 'scripts/i18n/limits.json'), 'utf8'))
const english = readCatalogue(join(STRINGS, 'en.ts'))

const cell = (v) => `"${String(v ?? '').replace(/"/g, '""')}"`
const row = (cells) => cells.map(cell).join(',')

function exportLocale(locale) {
  const path = join(STRINGS, `${locale}.ts`)
  const translated = existsSync(path)
    ? new Map(readCatalogue(path).map((e) => [e.key, e]))
    : new Map()
  const lines = [
    row([
      'key',
      'kind',
      'values',
      'context',
      'English',
      locale,
      'room',
      'status',
      'reviewer notes',
    ]),
  ]
  const counts = { missing: 0, same: 0, translated: 0 }
  for (const e of english) {
    const t = translated.get(e.key)
    const status = !t ? 'missing' : t.text === e.text ? 'same as English' : 'translated'
    counts[status === 'same as English' ? 'same' : status]++
    lines.push(
      row([
        e.key,
        e.kind,
        e.params.join(', '),
        e.context.join(' · '),
        e.text,
        t?.text ?? '',
        limits[e.key]?.room ?? '',
        status,
        '',
      ]),
    )
  }
  mkdirSync(OUT, { recursive: true })
  const file = join(OUT, `${locale}.csv`)
  // A byte-order mark, so spreadsheet apps read the file as UTF-8.
  writeFileSync(file, '﻿' + lines.join('\n') + '\n')
  return { file, ...counts }
}

const wanted = process.argv.slice(2)
for (const l of wanted) if (!LOCALES.includes(l)) throw new Error(`unknown language ${l}`)
const total = english.reduce((n, e) => n + words(e), 0)
console.log(`English: ${english.length} entries, about ${total} words`)
for (const l of wanted.length ? wanted : LOCALES) {
  const r = exportLocale(l)
  console.log(
    `${l}: ${r.translated} translated, ${r.same} same as English, ${r.missing} missing → ${r.file}`,
  )
}
