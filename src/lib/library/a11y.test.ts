import { describe, expect, it } from 'vitest'
import { accessibilityLines } from './a11y'

describe('E10 accessibility metadata in plain words', () => {
  it('translates known values, keeps summaries, dedupes, shows the unknown as is', () => {
    expect(
      accessibilityLines([
        ['accessMode', 'textual'],
        ['accessibilityFeature', 'alternativeText'],
        ['accessibilityFeature', 'pageBreakMarkers'],
        ['accessibilityFeature', 'printPageNumbers'],
        ['accessibilityHazard', 'none'],
        ['accessibilitySummary', 'Meets WCAG 2 AA.'],
        ['accessibilityFeature', 'somethingNew'],
      ]),
    ).toEqual([
      'Readable as text',
      'Has alternative text for images',
      'Marks the print pages',
      'No known hazards',
      'Meets WCAG 2 AA.',
      'Feature: somethingNew',
    ])
  })
})
