// Current keyboard-layout labels for shortcut hints (T7), refreshed when the
// window regains focus (the reader may have switched input source).

import { ipc } from './ipc'

let labels: ReadonlyMap<string, string> = new Map()

export const layoutLabels = () => labels

export async function refreshLayoutLabels() {
  try {
    labels = new Map(Object.entries(await ipc.keyboardLayoutLabels()))
  } catch {
    // Keep the previous labels; US defaults apply when none are known.
  }
}
