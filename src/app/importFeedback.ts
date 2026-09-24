// Messages for import outcomes. PROVISIONAL wording: the import states are part
// of the missing G4 designs (plan §10.4).

import type { ImportResult } from './ipc'
import type { MessageInput } from '../lib/reader/messages'

const fileName = (path: string) => path.split('/').pop() ?? path

export function importMessages(results: ImportResult[]): MessageInput[] {
  const out: MessageInput[] = []
  for (const { path, outcome } of results) {
    switch (outcome.kind) {
      case 'imported':
        if (outcome.damaged > 0)
          out.push({
            text: `“${outcome.title}” was added · ${outcome.damaged} damaged ${outcome.damaged === 1 ? 'chapter' : 'chapters'}`,
          })
        break
      case 'alreadyInLibrary':
        out.push({ text: `“${fileName(path)}” is already in your library` })
        break
      case 'replaced':
        out.push({ text: `“${outcome.title}” was updated from a newer file` })
        break
      case 'rejected':
        out.push({
          text: outcome.hostile
            ? `“${fileName(path)}” couldn’t be opened safely and wasn’t added`
            : `“${fileName(path)}” couldn’t be opened · ${outcome.reason}`,
          politeness: 'assertive',
        })
        break
    }
  }
  return out
}
