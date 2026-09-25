// E10: EPUB accessibility metadata (schema.org properties in the package) in plain
// words for the book info sheet. Unknown values are shown as they are.

const WORDS: Record<string, Record<string, string>> = {
  accessMode: {
    textual: 'Readable as text',
    visual: 'Has visual content',
    auditory: 'Has audio content',
  },
  accessModeSufficient: {
    textual: 'Can be read fully as text',
    visual: 'Needs sight for some content',
  },
  accessibilityFeature: {
    alternativeText: 'Has alternative text for images',
    longDescription: 'Has long descriptions for complex images',
    tableOfContents: 'Has a table of contents',
    readingOrder: 'Has a logical reading order',
    structuralNavigation: 'Has headings for navigation',
    pageBreakMarkers: 'Marks the print pages',
    printPageNumbers: 'Marks the print pages',
    MathML: 'Has mathematics as MathML',
    displayTransformability: 'Text can be restyled',
    synchronizedAudioText: 'Has read-aloud audio',
    ARIA: 'Uses ARIA roles',
    index: 'Has an index',
    captions: 'Has captions',
  },
  accessibilityHazard: {
    none: 'No known hazards',
    noFlashingHazard: 'No flashing hazard',
    noMotionSimulationHazard: 'No motion hazard',
    noSoundHazard: 'No sound hazard',
    flashing: 'Warning: flashing content',
    motionSimulation: 'Warning: motion simulation',
    sound: 'Warning: sound',
    unknown: 'Hazards unknown',
  },
}

export function accessibilityLines(meta: [string, string][]): string[] {
  const out: string[] = []
  for (const [prop, raw] of meta) {
    for (const value of raw
      .split(',')
      .map((v) => v.trim())
      .filter(Boolean)) {
      const line =
        prop === 'accessibilitySummary'
          ? value
          : (WORDS[prop]?.[value] ?? `${prop.replace(/^access(ibility)?/i, '') || prop}: ${value}`)
      if (!out.includes(line)) out.push(line)
    }
  }
  return out
}
