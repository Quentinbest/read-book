// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { InstalledExtension } from '../lib/extensions/types'

const core = vi.hoisted(() => ({ list: [] as InstalledExtension[], calls: [] as string[] }))
vi.mock('@tauri-apps/api/core', () => ({
  invoke: async (cmd: string) => {
    core.calls.push(cmd)
    if (cmd === 'app_safe_mode') return false
    if (cmd === 'extensions_list') return core.list
    return null
  },
}))
vi.mock('@tauri-apps/plugin-dialog', () => ({ save: async () => null }))

const { ExtensionHost } = await import('./host.svelte')

function installed(version: string): InstalledExtension {
  return {
    manifest: {
      id: 'org.example.tab',
      version,
      name: 'Tab',
      main: 'main.js',
      activation: ['onView:tab'],
    } as unknown as InstalledExtension['manifest'],
    enabled: true,
    granted: [],
    builtin: false,
    incompatible: null,
    suspended: false,
    crashes: [],
  }
}

const frames = () =>
  document.querySelectorAll<HTMLIFrameElement>('iframe[src^="linen-ext://org.example.tab/"]')

/** The host frames say they started, as `_host.html` does once the Worker runs. */
function ready() {
  for (const f of frames())
    window.dispatchEvent(
      new MessageEvent('message', {
        source: f.contentWindow,
        data: { linenExt: true, ready: true },
      }),
    )
}

describe('P4 extension workers and their packages', () => {
  beforeEach(() => {
    document.body.innerHTML = ''
    core.calls = []
  })

  it('an update stops the running worker of the old package', async () => {
    const host = new ExtensionHost()
    core.list = [installed('1.0.0')]
    await host.load()
    const started = host.activate('org.example.tab')
    ready()
    await started
    expect(host.isRunning('org.example.tab')).toBe(true)

    await host.load() // same package: it keeps running
    expect(host.isRunning('org.example.tab')).toBe(true)

    core.list = [installed('2.0.0')]
    await host.load()
    expect(host.isRunning('org.example.tab')).toBe(false)
    expect(frames()).toHaveLength(0)
  })

  it('an update restarts the worker of a showing tab with the new package', async () => {
    const host = new ExtensionHost()
    core.list = [installed('1.0.0')]
    await host.load()
    host.visibleTabs.add('org.example.tab')
    const started = host.activate('org.example.tab')
    ready()
    await started
    const first = frames()[0]

    core.list = [installed('2.0.0')]
    await host.load()
    expect(host.isRunning('org.example.tab')).toBe(true)
    expect(frames()).toHaveLength(1)
    expect(frames()[0]).not.toBe(first)
    host.unload('org.example.tab')
  })

  it('Restart starts the worker again while its tab shows', async () => {
    const host = new ExtensionHost()
    core.list = [installed('1.0.0')]
    await host.load()
    host.visibleTabs.add('org.example.tab')
    host.status['org.example.tab'] = 'not-responding'
    const restarted = host.restart('org.example.tab')
    await vi.waitFor(() => expect(frames()).toHaveLength(1))
    ready()
    await restarted
    expect(core.calls).toContain('extension_restart')
    expect(host.isRunning('org.example.tab')).toBe(true)
    expect(host.status['org.example.tab']).toBe('running')
    host.unload('org.example.tab')
  })
})

function withLookup(version: string): InstalledExtension {
  const x = installed(version)
  x.manifest = {
    ...x.manifest,
    activation: ['onLookup:explain'],
    contributes: {
      commands: [],
      selectionActions: [],
      navigatorTabs: [],
      themes: [],
      exporters: [],
      lookups: [{ id: 'explain', title: 'Explain', when: 'selection.words <= 3' }],
    },
  } as unknown as InstalledExtension['manifest']
  x.granted = ['book.selection']
  return x
}

const request = {
  text: 'cache',
  context: {
    before: '',
    sentence: 'The cache is warm.',
    after: '',
    paragraph: 'The cache is warm.',
    chapter: '',
    selection: { start: 4, end: 9 },
  },
  bookLang: 'en',
  language: 'en',
}

/** The Worker answers through its host frame. */
function answer(data: Record<string, unknown>) {
  window.dispatchEvent(
    new MessageEvent('message', {
      source: frames()[0].contentWindow,
      data: { linenExt: true, data },
    }),
  )
}

describe('LK1, LK4 lookups', () => {
  beforeEach(() => {
    document.body.innerHTML = ''
    core.calls = []
  })

  it('offers lookups whose when holds', async () => {
    const host = new ExtensionHost()
    core.list = [withLookup('1.0.0')]
    await host.load()
    const ctx = {
      'selection.words': 1,
      'selection.chars': 5,
      'selection.language': 'en',
      'book.language': 'en',
      'book.fixedLayout': false,
      'book.lang': 'en',
      'selection.sentences': 1,
    }
    expect(host.lookups(ctx).map((l) => l.id)).toEqual(['explain'])
    expect(host.lookups({ ...ctx, 'selection.words': 4 })).toEqual([])
  })

  it('answers a lookup, and drops a late answer to a cancelled one', async () => {
    const host = new ExtensionHost()
    core.list = [withLookup('1.0.0')]
    await host.load()
    const started = host.activate('org.example.tab')
    ready()
    await started
    const posted: unknown[] = []
    vi.spyOn(frames()[0].contentWindow!, 'postMessage').mockImplementation((m: unknown) => {
      posted.push(m)
    })

    const first = host.lookup('org.example.tab', 'explain', request, { text: 'cache', cfi: 'x' })
    await vi.waitFor(() => expect(posted).toHaveLength(1))
    expect(posted[0]).toMatchObject({ lookup: first.invocation, id: 'explain', request })
    answer({ done: first.invocation, result: { status: 'ok' } })
    await expect(first.result).resolves.toEqual({ status: 'ok' })

    const second = host.lookup('org.example.tab', 'explain', request, { text: 'cache', cfi: 'x' })
    await vi.waitFor(() => expect(posted).toHaveLength(2))
    host.cancelLookup('org.example.tab', second.invocation)
    expect(posted[2]).toEqual({ cancel: second.invocation })
    await expect(second.result).rejects.toThrow('cancelled')
    answer({ done: second.invocation, result: { status: 'ok' } })
    expect(host.status['org.example.tab']).toBe('running')
    expect(core.calls).not.toContain('extension_crashed')
    host.unload('org.example.tab')
  })

  it('never sends a lookup cancelled while its extension starts', async () => {
    const host = new ExtensionHost()
    core.list = [withLookup('1.0.0')]
    await host.load()
    const pending = host.lookup('org.example.tab', 'explain', request, { text: 'cache', cfi: 'x' })
    host.cancelLookup('org.example.tab', pending.invocation)
    await vi.waitFor(() => expect(frames()).toHaveLength(1))
    const posted: unknown[] = []
    vi.spyOn(frames()[0].contentWindow!, 'postMessage').mockImplementation((m: unknown) => {
      posted.push(m)
    })
    ready()
    await expect(pending.result).rejects.toThrow('cancelled')
    expect(posted.filter((m) => (m as { lookup?: number }).lookup)).toEqual([])
    host.unload('org.example.tab')
  })
})
