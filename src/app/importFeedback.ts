// Messages for import outcomes (wording in the catalogue; PROVISIONAL until G4).

import { t } from '../lib/strings'
import type { MessageInput } from '../lib/reader/messages'
import type { ImportResult } from './ipc'

/** Why a file was rejected, in the UI language; an unknown code shows the core's text. */
function reasonText(code: string, args: string[], fallback: string): string {
  const m = (t.import.reasons as Record<string, string | ((...a: string[]) => string)>)[code]
  return typeof m === 'function' ? m(...args) : (m ?? fallback)
}

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
              : t.import.rejected(path, reasonText(outcome.code, outcome.args, outcome.reason)),
          politeness: 'assertive',
        })
        break
    }
  }
  return out
}
