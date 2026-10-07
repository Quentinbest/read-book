#!/usr/bin/env node
// Start a language's catalogue from the English one (docs/i18n/README.md):
//
//   node scripts/i18n/new-locale.mjs ja      → src/lib/strings/ja.ts
//
// The copy keeps every key, comment and function, so the translator replaces the
// English text in place and the type check catches anything lost. It is not yet
// offered in Linen: registering it (index.ts, Info.plist) is a separate step.

import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

const LOCALES = { 'zh-Hans': 'zhHans', 'zh-Hant': 'zhHant', ja: 'ja', es: 'es' }
const NAMES = {
  'zh-Hans': 'Simplified Chinese',
  'zh-Hant': 'Traditional Chinese (Taiwan usage, L-2)',
  ja: 'Japanese',
  es: 'Spanish (neutral, tú; L-3)',
}

const locale = process.argv[2]
const ident = LOCALES[locale]
if (!ident) throw new Error(`usage: new-locale.mjs ${Object.keys(LOCALES).join('|')}`)

const strings = join(new URL('../..', import.meta.url).pathname, 'src/lib/strings')
const target = join(strings, `${locale}.ts`)
if (existsSync(target)) throw new Error(`${target} exists; edit it instead`)

const en = readFileSync(join(strings, 'en.ts'), 'utf8')
const header = en.indexOf('export const en = {')
const footer = en.lastIndexOf('} as const')
if (header < 0 || footer < 0) throw new Error('en.ts no longer has the expected shape')

const body = en
  .slice(0, footer)
  .replace(/^[\s\S]*?(?=^import )/m, '')
  .replace('export const en = {', `export const ${ident}: Messages = {`)

writeFileSync(
  target,
  `// The ${NAMES[locale]} catalogue (L-1). Started from en.ts by
// scripts/i18n/new-locale.mjs: replace each English text in place, keep every
// \`\${…}\` value, and follow docs/i18n/style.md and docs/i18n/glossary.md.
// Strings still equal to English are listed by src/lib/strings/catalogues.test.ts.

import type { Messages } from './types'
${body}}
`,
)
console.log(`wrote ${target}`)
