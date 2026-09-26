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
