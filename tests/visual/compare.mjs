// Compare fresh app captures (docs/visual/app, from spike 'v') with the approved
// baselines (plan §6.1). Exit 1 if any screen differs beyond font rasterisation noise.
// Run: node tests/visual/compare.mjs [webkit-19618]
import { readdirSync, readFileSync, writeFileSync, mkdirSync } from 'node:fs'
import pixelmatch from 'pixelmatch'
import { PNG } from 'pngjs'

const engine = process.argv[2] ?? 'webkit-19618'
const baseDir = `docs/visual/baselines/${engine}`
const appDir = 'docs/visual/app'
const diffDir = 'docs/visual/diff'
/** Share of pixels allowed to differ: anti-aliasing noise only (threshold 0.1 per pixel). */
const MAX_CHANGED = 0.001

mkdirSync(diffDir, { recursive: true })
let failed = 0
for (const name of readdirSync(baseDir).filter((f) => f.endsWith('.png'))) {
  const base = PNG.sync.read(readFileSync(`${baseDir}/${name}`))
  let now
  try {
    now = PNG.sync.read(readFileSync(`${appDir}/${name}`))
  } catch {
    console.log(`MISSING ${name}: no fresh capture (run spike 'v')`)
    failed++
    continue
  }
  if (base.width !== now.width || base.height !== now.height) {
    console.log(
      `FAIL ${name}: size ${now.width}×${now.height}, baseline ${base.width}×${base.height}`,
    )
    failed++
    continue
  }
  const diff = new PNG({ width: base.width, height: base.height })
  const changed = pixelmatch(base.data, now.data, diff.data, base.width, base.height, {
    threshold: 0.1,
  })
  const share = changed / (base.width * base.height)
  const ok = share <= MAX_CHANGED
  if (!ok) {
    writeFileSync(`${diffDir}/${name}`, PNG.sync.write(diff))
    failed++
  }
  console.log(
    `${ok ? 'PASS' : 'FAIL'} ${name}: ${changed} pixels differ (${(share * 100).toFixed(3)}%)`,
  )
}
process.exit(failed ? 1 : 0)
