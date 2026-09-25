// Phase 7 Done-when: Markdown Export output matches a golden file. The built-in
// extension's own script runs here with a stand-in for the Host API.
import { describe, expect, it } from 'vitest'
import source from '../../src-tauri/src/extensions/builtin/markdown-export/main.js?raw'
import input from './golden/markdown-export.input.json'
import expected from './golden/markdown-export.expected.md?raw'

function load() {
  const self: { markdownExport?: { markdown(c: unknown): string } } = {}
  const linen = { commands: { register: () => Promise.resolve(true) } }
  new Function('self', 'linen', source)(self, linen)
  return self.markdownExport!
}

describe('Markdown Export (built in)', () => {
  it('matches the golden file', () => {
    expect(load().markdown(input)).toBe(expected)
  })
})
