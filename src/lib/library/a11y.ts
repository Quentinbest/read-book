// E10: EPUB accessibility metadata (schema.org properties in the package) in plain
// words for the book info sheet (the catalogue's `info.a11y`). Unknown values are
// shown as they are.

import { t } from '../strings'

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
          : ((t.info.a11y as Record<string, Record<string, string>>)[prop]?.[value] ??
            `${prop.replace(/^access(ibility)?/i, '') || prop}: ${value}`)
      if (!out.includes(line)) out.push(line)
    }
  }
  return out
}
