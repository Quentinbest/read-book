import { describe, expect, it, vi } from 'vitest'
import { distinctLookupTitles, lookupProviders } from './providers'

const lookup = (extId: string, title: string, name: string, id = 'explain') => ({
  extId,
  id,
  title,
  name,
})

describe('distinctLookupTitles (SB1)', () => {
  it('keeps titles that no other lookup shares', () => {
    const titles = distinctLookupTitles([
      lookup('io.example.explain', 'Explain', 'Explain'),
      lookup('org.example.dictionary', 'Free Dictionary', 'Dictionary', 'define'),
    ])
    expect(titles.get('io.example.explain/explain')).toBe('Explain')
    expect(titles.get('org.example.dictionary/define')).toBe('Free Dictionary')
  })

  it('names the extension when two lookups share a title; one named like it stays plain', () => {
    // The owner's library (item 93): the harness's provider and Explain.
    const titles = distinctLookupTitles([
      lookup('test.lookup', 'Explain', 'Test Lookup'),
      lookup('io.github.quentinbest.explain', 'Explain', 'Explain'),
    ])
    expect(titles.get('test.lookup/explain')).toBe('Explain (Test Lookup)')
    expect(titles.get('io.github.quentinbest.explain/explain')).toBe('Explain')
  })

  it('names every extension when none is named like the title', () => {
    const titles = distinctLookupTitles([
      lookup('a.one', 'Explain', 'Lens'),
      lookup('b.two', 'Explain', 'Gloss'),
    ])
    expect(titles.get('a.one/explain')).toBe('Explain (Lens)')
    expect(titles.get('b.two/explain')).toBe('Explain (Gloss)')
  })

  it('ignores case and surrounding space when comparing titles', () => {
    const titles = distinctLookupTitles([
      lookup('a.one', 'explain', 'Lens'),
      lookup('b.two', 'Explain ', 'Gloss'),
    ])
    expect(titles.get('a.one/explain')).toBe('explain (Lens)')
    expect(titles.get('b.two/explain')).toBe('Explain  (Gloss)')
  })

  it('compares titles the same way whatever the system locale (Turkish dotted I)', () => {
    // Under a Turkish locale, toLocaleLowerCase() makes “I” into a dotless “ı”.
    const own = String.prototype.toLocaleLowerCase
    const spy = vi.spyOn(String.prototype, 'toLocaleLowerCase').mockImplementation(function (
      this: string,
    ) {
      return own.call(this, 'tr')
    })
    try {
      const titles = distinctLookupTitles([
        lookup('a.one', 'INFO', 'Lens'),
        lookup('b.two', 'info', 'Gloss'),
      ])
      expect(titles.get('a.one/explain')).toBe('INFO (Lens)')
      expect(titles.get('b.two/explain')).toBe('info (Gloss)')
    } finally {
      spy.mockRestore()
    }
  })

  it('falls back to the extension ID when names clash too, then to the lookup', () => {
    const named = distinctLookupTitles([
      lookup('a.one', 'Explain', 'Explain'),
      lookup('b.two', 'Explain', 'Explain'),
    ])
    expect(named.get('a.one/explain')).toBe('Explain (a.one)')
    expect(named.get('b.two/explain')).toBe('Explain (b.two)')
    const same = distinctLookupTitles([
      lookup('a.one', 'Explain', 'Lens', 'short'),
      lookup('a.one', 'Explain', 'Lens', 'long'),
    ])
    expect(same.get('a.one/short')).toBe('Explain (a.one/short)')
    expect(same.get('a.one/long')).toBe('Explain (a.one/long)')
  })

  it('never shows two lookups under one label', () => {
    const lookups = [
      lookup('test.lookup', 'Explain', 'Test Lookup'),
      lookup('io.github.quentinbest.explain', 'Explain', 'Explain'),
      lookup('c.three', 'Explain', 'Test Lookup'),
      lookup('d.four', 'Free Dictionary', 'Dictionary', 'define'),
    ]
    const shown = [...distinctLookupTitles(lookups).values()].map((s) => s.trim().toLowerCase())
    expect(new Set(shown).size).toBe(lookups.length)
  })
})

describe('lookupProviders (SB4)', () => {
  it('carries a lookup’s icon to each of its languages', () => {
    const providers = lookupProviders(
      [
        { extId: 'a.one', id: 'explain', title: 'Explain', name: 'Explain', icon: 'lightbulb' },
        { extId: 'b.two', id: 'define', title: 'Define', name: 'Dictionary' },
      ],
      { languages: ['en', 'zh-Hans'], mac: true },
    )
    expect(providers.filter((p) => p.icon === 'lightbulb').map((p) => p.key)).toEqual([
      'a.one/explain@en',
      'a.one/explain@zh-Hans',
    ])
    expect(providers.find((p) => p.extId === 'b.two')?.icon).toBeNull()
  })
})
