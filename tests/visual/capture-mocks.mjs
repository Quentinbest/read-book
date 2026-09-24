// Capture the approved screen mocks (docs/design) as PNGs for side-by-side review
// with the app (plan §6.1). Run: node tests/visual/capture-mocks.mjs
import { chromium } from '@playwright/test'
import { pathToFileURL } from 'node:url'
import { resolve } from 'node:path'

const boards = {
  '02': '02 Reader — immersive',
  '03': '03 Reader — controls visible',
  10: '10 Reading themes',
  14: '14 Night — controls and selection',
}
const file = pathToFileURL(resolve('docs/design/Quiet EPUB Reader (screens).html')).href
const browser = await chromium.launch()
const page = await browser.newPage({
  viewport: { width: 1600, height: 1000 },
  deviceScaleFactor: 1,
})
await page.goto(file)
for (const [id, title] of Object.entries(boards)) {
  const frame = page.locator(`iframe[title="${title}"]`)
  await frame.waitFor({ state: 'attached', timeout: 60_000 })
  await frame.scrollIntoViewIfNeeded()
  // Nested bundles render asynchronously; wait until the board has painted its text.
  await page.waitForTimeout(2500)
  await frame.screenshot({ path: `docs/visual/mocks/${id}.png` })
  console.log(`captured mock ${id}`)
}
await browser.close()
