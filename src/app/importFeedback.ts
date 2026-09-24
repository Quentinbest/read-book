// Messages for import outcomes (wording in the catalogue; PROVISIONAL until G4).

import { t } from '../lib/strings/en'
import type { MessageInput } from '../lib/reader/messages'
import type { ImportResult } from './ipc'

export function importMessages(results: ImportResult[]): MessageInput[] {
  const out: MessageInput[] = []
  for (const { path, outcome } of results) {
    switch (outcome.kind) {
      case 'imported':
        if (outcome.damaged > 0)
          out.push({ text: t.import.addedDamaged(outcome.title, outcome.damaged) })
        break
      case 'alreadyInLibrary':
        out.push({ text: t.import.alreadyInLibrary(path) })
        break
      case 'replaced':
        out.push({ text: t.import.replaced(outcome.title) })
        break
      case 'rejected':
        out.push({
          text: outcome.drm
            ? t.import.drm(path)
            : outcome.hostile
              ? t.import.hostile(path)
              : t.import.rejected(path, outcome.reason),
          politeness: 'assertive',
        })
        break
    }
  }
  return out
}
