import { afterEach, describe, expect, it, vi } from 'vitest'

// Record what the menu bar would ask Tauri for: each item's text, in order.
const made = vi.hoisted(() => ({ texts: [] as string[] }))
vi.mock('@tauri-apps/api/menu', () => {
  const item = (o: { text?: string; item?: unknown }) => {
    if (o.text !== undefined) made.texts.push(o.text)
    else if (o.item !== 'Separator') made.texts.push(`<untitled ${JSON.stringify(o.item)}>`)
    return Promise.resolve({})
  }
  return {
    Menu: { new: () => Promise.resolve({ setAsAppMenu: () => Promise.resolve() }) },
    MenuItem: { new: item },
    PredefinedMenuItem: { new: item },
    Submenu: { new: item },
  }
})

const { installMenuBar } = await import('./menubar')
const { CommandRegistry } = await import('../lib/commands/registry')
const strings = await import('../lib/strings')

async function texts(): Promise<string[]> {
  made.texts = []
  const r = new CommandRegistry()
  for (const id of ['app.settings', 'book.open', 'search.next', 'chapter.next', 'library.show'])
    r.handle(id, { run() {} })
  await installMenuBar(r)
  return made.texts
}

describe('L-1 the menu bar in the UI language', () => {
  afterEach(() => strings.setLocale('en'))

  it('English names AppKit’s standard items as macOS does', async () => {
    const all = await texts()
    for (const name of [
      'About Linen',
      'Services',
      'Hide Linen',
      'Hide Others',
      'Show All',
      'Quit Linen',
      'Cut',
      'Copy',
      'Paste',
      'Select All',
      'Minimize',
      'File',
      'Edit',
    ])
      expect(all).toContain(name)
    expect(all.filter((x) => x.startsWith('<untitled'))).toEqual([])
  })

  it('every item but the app’s name follows the language (en-XA)', async () => {
    strings.setLocale('en-XA')
    const all = await texts()
    expect(all.filter((x) => x !== 'Linen' && !x.startsWith('⟦'))).toEqual([])
    expect(all).toContain('Linen')
  })
})
