import { describe, expect, it } from 'vitest'
import { contributionLabels, permissionLabel } from './labels'
import type { ExtensionManifest } from './types'

describe('P3 permissions in plain words (Screen 11)', () => {
  it('names each permission and marks the ones P3 highlights', () => {
    expect(permissionLabel('book.selection')).toEqual({
      label: 'Read the text you select',
      warn: false,
    })
    expect(permissionLabel('network:api.dictionaryapi.dev')).toMatchObject({
      label: 'Connect to api.dictionaryapi.dev',
      network: true,
      warn: false,
    })
    expect(permissionLabel('network:*.example.org').warn).toBe(true)
    expect(permissionLabel('book.text').warn).toBe(true)
    expect(permissionLabel('background').label).toBe('Run in the background')
  })
  it('lists what an extension adds', () => {
    const m = {
      contributes: {
        commands: [{ id: 'define', title: 'Define' }],
        selectionActions: [{ command: 'define' }],
        navigatorTabs: [{ id: 't', title: 'Definitions', page: 'tab.html' }],
        themes: [],
        exporters: [],
      },
    } as unknown as ExtensionManifest
    expect(contributionLabels(m)).toEqual([
      'Command',
      '“Define” in selection menu',
      'Navigator tab',
    ])
  })
})
