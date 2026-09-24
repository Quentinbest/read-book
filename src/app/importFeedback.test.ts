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
          reason: 'encrypted content (DRM) in c1',
          hostile: false,
          drm: true,
        },
      },
      {
        path: '/x/evil.epub',
        outcome: { kind: 'rejected', reason: 'unsafe entry path ../x', hostile: true, drm: false },
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
})
