import { describe, expect, it } from 'vitest'
import { importMessages } from './importFeedback'

describe('import messages', () => {
  it('names DRM-protected files (B9) and hostile files, and stays quiet for clean imports', () => {
    const texts = importMessages([
      {
        path: '/x/clean.epub',
        outcome: { kind: 'imported', book_id: '1', title: 'Clean', damaged: 0 },
      },
      {
        path: '/x/locked.epub',
        outcome: {
          kind: 'rejected',
          code: 'drm',
          args: ['c1'],
          reason: 'encrypted content (DRM) in c1',
          hostile: false,
          drm: true,
        },
      },
      {
        path: '/x/evil.epub',
        outcome: {
          kind: 'rejected',
          code: 'unsafe_path',
          args: ['../x'],
          reason: 'unsafe entry path ../x',
          hostile: true,
          drm: false,
        },
      },
      {
        path: '/x/part.epub',
        outcome: { kind: 'imported', book_id: '2', title: 'Part', damaged: 1 },
      },
    ]).map((m) => m.text)
    expect(texts).toEqual([
      '“locked.epub” is protected by DRM and can’t be opened in Linen',
      '“evil.epub” couldn’t be opened safely and wasn’t added',
      '“Part” was added · 1 damaged chapter',
    ])
  })
  it('words other rejections by their code (L-1), or shows the core’s text for an unknown one', () => {
    const rejected = (code: string, args: string[], reason: string) => ({
      path: '/x/a.epub',
      outcome: { kind: 'rejected' as const, code, args, reason, hostile: false, drm: false },
    })
    const texts = importMessages([
      rejected('compression_ratio', ['big.xhtml', '100'], ''),
      rejected('no_chapters', [], ''),
      rejected('from_a_newer_core', [], 'something new'),
    ]).map((m) => m.text)
    expect(texts).toEqual([
      '“a.epub” couldn’t be opened · entry big.xhtml compresses more than 100:1',
      '“a.epub” couldn’t be opened · none of the book’s chapters could be opened',
      '“a.epub” couldn’t be opened · something new',
    ])
  })
})
