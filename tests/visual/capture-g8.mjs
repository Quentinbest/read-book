// Capture the G8 design proposals (docs/design/g8/proposals.html) as PNGs, with
// and without the numbered notes. Run: node tests/visual/capture-g8.mjs
import { chromium } from '@playwright/test'
import { mkdirSync } from 'node:fs'
import { resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

const out = 'docs/design/g8/png'
mkdirSync(out, { recursive: true })
const browser = await chromium.launch()
const page = await browser.newPage({
  viewport: { width: 1800, height: 1200 },
  deviceScaleFactor: 1,
})
await page.goto(pathToFileURL(resolve('docs/design/g8/proposals.html')).href)
await page.evaluate(() => document.fonts.ready)
const ids = await page.$$eval('.frame', (els) => els.map((e) => e.id))
for (const id of ids) {
  const frame = page.locator(`#${id}`)
  await frame.screenshot({ path: `${out}/${id}.png` })
  await frame.evaluate((el) => el.classList.add('clean'))
  await frame.screenshot({ path: `${out}/${id}-clean.png` })
  console.log(`captured ${id}`)
}
await browser.close()
