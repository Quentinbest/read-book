import { describe, expect, it } from 'vitest'
import { validateResult } from './result'
import {
  lookupProviders,
  providerLabel,
  providerMenu,
  readerLanguages,
  sourceLabel,
} from './providers'

const ok = {
  status: 'ok',
  headword: 'invalidates',
  term: '使（缓存）失效',
  meaning: '写入提交后，让受影响的缓存条目作废。',
  qualifier: '不是删除数据。',
  details: [{ label: 'Why this reading', text: 'The sentence before mentions a cache.' }],
  source: { kind: 'ai', name: 'DeepSeek', model: 'deepseek-chat' },
  sent: 'Sentence: When a write commits, 50% of …',
}

describe('LK2 result validator', () => {
  it('accepts the fixed fields', () => {
    const v = validateResult(ok)
    expect(v.ok).toBe(true)
    if (v.ok)
      expect(v.result.source).toEqual({ kind: 'ai', name: 'DeepSeek', model: 'deepseek-chat' })
  })

  it('refuses unknown fields and labels of the provider’s own (EQ1)', () => {
    expect(validateResult({ ...ok, verified: true })).toMatchObject({ ok: false })
    expect(
      validateResult({ ...ok, source: { ...ok.source, label: 'Verified source' } }),
    ).toMatchObject({
      ok: false,
    })
    expect(validateResult({ ...ok, source: { kind: 'oracle', name: 'X' } })).toMatchObject({
      ok: false,
    })
    expect(
      validateResult({ ...ok, details: [{ label: 'a', text: 'b', html: '<b>' }] }),
    ).toMatchObject({
      ok: false,
    })
  })

  it('refuses percentages and confidence scores (EQ2), but not in what was sent', () => {
    expect(validateResult({ ...ok, meaning: 'Means to void (92% sure).' })).toMatchObject({
      ok: false,
    })
    expect(validateResult({ ...ok, qualifier: 'Confidence: high' })).toMatchObject({ ok: false })
    expect(validateResult({ ...ok, details: [{ label: 'Score', text: '87 %' }] })).toMatchObject({
      ok: false,
    })
    expect(validateResult(ok).ok).toBe(true)
  })

  it('refuses markup: providers return plain text (only the core dictionary may return HTML)', () => {
    expect(validateResult({ ...ok, meaning: '<img src=x onerror=alert(1)>' })).toMatchObject({
      ok: false,
    })
    expect(validateResult({ ...ok, meaning: 'a < b and c > d' }).ok).toBe(true)
  })

  it('checks each status’s own fields (EQ4, LK10)', () => {
    const base = { headword: 'x', source: { kind: 'ai', name: 'DeepSeek' } }
    expect(validateResult({ ...base, status: 'needs_context' })).toMatchObject({ ok: false })
    expect(
      validateResult({ ...base, status: 'needs_context', missing: 'Defined in chapter 3.' }).ok,
    ).toBe(true)
    expect(validateResult({ ...base, status: 'error', error: 'offline' }).ok).toBe(true)
    expect(validateResult({ ...base, status: 'error', error: 'kaboom' })).toMatchObject({
      ok: false,
    })
    expect(validateResult({ ...base, status: 'ok', meaning: 'm', error: 'offline' })).toMatchObject(
      {
        ok: false,
      },
    )
    expect(validateResult({ ...base, status: 'ok' })).toMatchObject({ ok: false })
  })

  it('refuses what is too long or not text', () => {
    expect(validateResult({ ...ok, meaning: 'x'.repeat(2001) })).toMatchObject({ ok: false })
    expect(validateResult({ ...ok, headword: 7 })).toMatchObject({ ok: false })
    expect(validateResult(null)).toMatchObject({ ok: false })
    expect(validateResult([ok])).toMatchObject({ ok: false })
  })
})

describe('item 76: the first-request notice', () => {
  const notice = {
    status: 'notice',
    headword: 'invalidates',
    notice: {
      title: 'Explain sends text to DeepSeek',
      text: 'To explain this, Explain sends the sentence and one on each side.',
      host: 'api.deepseek.com',
    },
    source: { kind: 'ai', name: 'DeepSeek' },
  }
  it('accepts a notice with a title, its text and the host', () => {
    const v = validateResult(notice)
    expect(v.ok).toBe(true)
    if (v.ok) expect(v.result.notice?.host).toBe('api.deepseek.com')
  })
  it('refuses a notice without a host, with markup, or outside its status', () => {
    expect(
      validateResult({ ...notice, notice: { ...notice.notice, host: 'https://x.org/' } }),
    ).toMatchObject({ ok: false })
    expect(
      validateResult({ ...notice, notice: { ...notice.notice, text: '<a href=x>ok</a>' } }),
    ).toMatchObject({ ok: false })
    expect(
      validateResult({ ...notice, notice: { ...notice.notice, button: 'Accept' } }),
    ).toMatchObject({ ok: false })
    expect(validateResult({ ...notice, status: 'ok', meaning: 'm' })).toMatchObject({ ok: false })
  })
})

describe('LK11, LK15, EQ1 providers', () => {
  const lookups = [{ extId: 'org.test.explain', id: 'explain', title: 'Explain', name: 'Explain' }]

  it('offers an extension lookup in each language the reader uses, then this Mac', () => {
    const p = lookupProviders(lookups, { languages: readerLanguages('zh-Hans'), mac: true })
    expect(p.map((x) => x.key)).toEqual([
      'org.test.explain/explain@zh-Hans',
      'org.test.explain/explain@en',
      'mac',
    ])
    expect(providerLabel(p[0], p)).toBe('Explain · 简体中文')
    expect(providerLabel(p[1], p)).toBe('Explain · English')
    expect(readerLanguages('en')).toEqual(['en'])
  })

  it('is plain text when one provider applies (LK15)', () => {
    const mac = lookupProviders([], { languages: ['en'], mac: true })
    expect(providerMenu(mac, 'mac').plain).toBe(true)
    const one = lookupProviders(lookups, { languages: ['en'], mac: false })
    expect(providerMenu(one, one[0].key).plain).toBe(true)
    expect(providerLabel(one[0], one)).toBe('Explain')
    const two = lookupProviders(lookups, { languages: ['en'], mac: true })
    const menu = providerMenu(two, 'mac')
    expect(menu.plain).toBe(false)
    expect(menu.items.find((i) => i.checked)?.key).toBe('mac')
  })

  it('labels answers from a closed set (EQ1)', () => {
    const [explain, mac] = lookupProviders(lookups, { languages: ['en'], mac: true })
    expect(
      sourceLabel({ source: { kind: 'ai', name: 'DeepSeek', model: 'deepseek-chat' } }, explain),
    ).toBe('AI explanation · deepseek-chat')
    expect(sourceLabel({ source: { kind: 'ai', name: 'Ollama' } }, explain)).toBe(
      'AI explanation · Ollama',
    )
    expect(sourceLabel({ source: { kind: 'dictionary', name: 'Wiktionary' } }, explain)).toBe(
      'Dictionary · Wiktionary',
    )
    expect(sourceLabel(null, mac)).toBe('Dictionary · on this Mac')
    for (const label of [
      sourceLabel(null, mac),
      sourceLabel({ source: ok.source as never }, explain),
    ])
      expect(label).not.toMatch(/verified|source/i)
  })
})
