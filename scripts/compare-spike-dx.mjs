// Anchors across engines (next-steps plan, Phase 10): every engine picked the same
// seeded ranges; the same text must give the same CFI everywhere.
// Usage: node scripts/compare-spike-dx.mjs <dir with dx-anchors-*.json>
import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

const dir = process.argv[2] ?? 'docs/spikes/raw'
const runs = Object.fromEntries(
  readdirSync(dir, { recursive: true })
    .filter((f) => /dx-anchors-(macos|windows|linux)\.json$/.test(f))
    .map((f) => {
      const r = JSON.parse(readFileSync(join(dir, f), 'utf8'))
      return [r.spike.replace('dx-anchors-', ''), r]
    }),
)
const base = runs.macos
if (!base) {
  console.log('No macOS run to compare against.')
  process.exit(1)
}
const byN = new Map(base.raw.anchors.map((a) => [a.n, a]))
let failed = false
const out = [
  '| Engine | Self round-trip | Same range picked | Same CFI for the same text | Different CFI or text |',
  '|---|---|---|---|---|',
]
const notes = []
for (const [name, run] of Object.entries(runs)) {
  const self = run.raw.anchors.filter((a) => a.text === a.resolved).length
  let sameText = 0
  let sameCfi = 0
  const diffs = []
  for (const a of run.raw.anchors) {
    const b = byN.get(a.n)
    if (!b) continue
    if (a.text === b.text) {
      sameText++
      if (a.cfi === b.cfi) sameCfi++
      else diffs.push(`#${a.n} CFI ${a.cfi} vs ${b.cfi}`)
    } else
      diffs.push(
        `#${a.n} text ${JSON.stringify(a.text.slice(0, 40))} vs ${JSON.stringify(b.text.slice(0, 40))}`,
      )
  }
  const ok = self === run.raw.anchors.length && sameCfi === base.raw.anchors.length
  if (!ok) failed = true
  out.push(
    `| ${name} | ${self}/${run.raw.anchors.length} | ${sameText}/${base.raw.anchors.length} | ${sameCfi}/${sameText} | ${diffs.length} |`,
  )
  if (diffs.length) notes.push('', `**${name}:**`, ...diffs.slice(0, 10).map((d) => `- ${d}`))
}
console.log([...out, ...notes].join('\n'))
process.exit(failed ? 1 : 0)
