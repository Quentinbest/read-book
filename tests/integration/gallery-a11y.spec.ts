import AxeBuilder from '@axe-core/playwright'
import { expect, test } from '@playwright/test'

// Plan Phase 1 Done-when: axe reports no serious or critical violations on the
// component gallery. Checked in every theme, with and without a modal open.
for (const theme of ['paper', 'sepia', 'night'] as const) {
  for (const modal of [false, true]) {
    test(`gallery in ${theme}${modal ? ' with a modal' : ''} has no serious or critical axe violations`, async ({
      page,
    }) => {
      await page.goto(`/gallery.html?theme=${theme}${modal ? '&modal' : ''}`)
      await expect(page.getByRole('heading', { name: 'Component gallery' })).toBeVisible()
      const results = await new AxeBuilder({ page })
        .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'])
        .analyze()
      const serious = results.violations.filter(
        (v) => v.impact === 'serious' || v.impact === 'critical',
      )
      expect(
        serious.map((v) => `${v.id}: ${v.nodes.map((n) => n.target.join(' ')).join(', ')}`),
      ).toEqual([])
    })
  }
}

test('segmented control and tabs follow arrow keys', async ({ page }) => {
  await page.goto('/gallery.html')
  const paper = page.getByRole('radio', { name: 'Paper' })
  await paper.focus()
  await page.keyboard.press('ArrowRight')
  await expect(page.getByRole('radio', { name: 'Sepia' })).toBeFocused()
  await expect(page.getByRole('radio', { name: 'Sepia' })).toHaveAttribute('aria-checked', 'true')
  await page.getByRole('tab', { name: 'Contents' }).focus()
  await page.keyboard.press('End')
  await expect(page.getByRole('tab', { name: 'Notes' })).toHaveAttribute('aria-selected', 'true')
})

test('the modal traps focus and closes on Esc (S8)', async ({ page }) => {
  await page.goto('/gallery.html')
  await page.getByRole('button', { name: 'Open modal' }).click()
  await page.getByRole('textbox', { name: 'Command' }).focus()
  const focusInDialog = () =>
    page.evaluate(
      () => document.querySelector('dialog[open]')?.contains(document.activeElement) ?? false,
    )
  for (let i = 0; i < 3; i++) {
    await page.keyboard.press('Tab')
    expect(await focusInDialog()).toBe(true)
  }
  // The page behind the modal is inert.
  expect(
    await page.evaluate(() => document.querySelector('main')!.matches(':not(:has(:focus))')),
  ).toBe(true)
  await page.keyboard.press('Escape')
  await expect(page.locator('dialog[open]')).toHaveCount(0)
})

test('icon buttons have 44 px hit areas (X4)', async ({ page }) => {
  await page.goto('/gallery.html')
  for (const name of ['Library', 'Contents', 'Search', 'More']) {
    const box = await page.getByRole('button', { name, exact: true }).boundingBox()
    expect(box!.width).toBeGreaterThanOrEqual(44)
    expect(box!.height).toBeGreaterThanOrEqual(44)
  }
})
