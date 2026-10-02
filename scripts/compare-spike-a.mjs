// Spike A parity (next-steps plan, Phase 10): compare page counts per chapter and
// size between engines, against the macOS (WKWebView) run of the same workflow.
// Usage: node scripts/compare-spike-a.mjs <dir with a-rendering-*.json>
// Exits 1 when any engine misses a criterion; prints a Markdown report.
import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

const LIMIT = 0.02
const dir = process.argv[2] ?? 'docs/spikes/raw'
const runs = Object.fromEntries(
  readdirSync(dir, { recursive: true })
    .filter((f) => /a-rendering-(macos|windows|linux)\.json$/.test(f))
    .map((f) => {
      const r = JSON.parse(readFileSync(join(dir, f), 'utf8'))
      return [r.spike.replace('a-rendering-', ''), r]
    }),
)
const base = runs.macos
if (!base) {
  console.log('No macOS run to compare against.')
  process.exit(1)
}
const key = (c) => `${c.fontPx}px ch${c.chapter}`
const basePages = new Map(base.raw.checks.map((c) => [key(c), c.pages]))
const verdict = (ok) => (ok ? 'pass' : 'FAIL')
let failed = false
const out = [
  '| Engine | Literata | Split lines | Layouts | Worst page-count difference | Parity (≤ 2%) |',
  '|---|---|---|---|---|---|',
]
for (const [name, run] of Object.entries(runs)) {
  const crit = Object.fromEntries(run.criteria.map((c) => [c.id, c.verdict]))
  const split = run.raw.checks.reduce((a, c) => a + c.splitLines, 0)
  let worst = 0
  let worstAt = '—'
  let missing = 0
  for (const c of run.raw.checks) {
    const b = basePages.get(key(c))
    if (b === undefined) {
      missing++
      continue
    }
    const d = Math.abs(c.pages - b) / b
    if (d > worst) [worst, worstAt] = [d, `${key(c)}: ${c.pages} vs ${b}`]
  }
  const parity = worst <= LIMIT && missing === 0
  const literata = crit['A-literata'] === 'pass'
  if (!parity || !literata || split > 0) failed = true
  out.push(
    `| ${name} | ${verdict(literata)} | ${split} | ${run.raw.checks.length - missing}/${run.raw.checks.length} | ${(worst * 100).toFixed(1)}% (${worstAt}) | ${verdict(parity)} |`,
  )
}
for (const [name, run] of Object.entries(runs))
  out.push('', `- **${name}:** ${run.raw.engine ?? 'engine not recorded'}`)
console.log(out.join('\n'))
process.exit(failed ? 1 : 0)
