import { describe, expect, it } from 'vitest'
import { clampTextSize, stepTextSize, TEXT_SIZES } from './textSizes'

describe('L4 text sizes', () => {
  it('twelve steps from 14 to 32 px, 19 px among them', () => {
    expect(TEXT_SIZES).toHaveLength(12)
    expect(TEXT_SIZES[0]).toBe(14)
    expect(TEXT_SIZES.at(-1)).toBe(32)
    expect(TEXT_SIZES).toContain(19)
  })
  it('⌘+ goes past 32 up to 48 px; ⌘− down to 14 px', () => {
    expect(stepTextSize(19, 1)).toBe(20)
    expect(stepTextSize(32, 1)).toBe(36)
    expect(stepTextSize(48, 1)).toBe(48)
    expect(stepTextSize(14, -1)).toBe(14)
    expect(stepTextSize(21, -1)).toBe(20) // off the steps: to the next one down
  })
  it('stored sizes are kept in range', () => {
    expect(clampTextSize(100)).toBe(48)
    expect(clampTextSize(3)).toBe(14)
    expect(clampTextSize(NaN)).toBe(19)
  })
})
