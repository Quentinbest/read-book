// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import { computeChunks, showChunk } from '../../reader/chunks'
import { CONTEXT_MAX, contextLength, contextOf, selectionContext, sentenceCount } from './context'

function doc(body: string) {
  return new DOMParser().parseFromString(
    `<html xmlns:epub="http://www.idpf.org/2007/ops"><head></head><body>${body}</body></html>`,
    'text/html',
  )
}

/** A range over the `n`th occurrence (0-based) of `word` in the document's text nodes. */
function select(d: Document, word: string, n = 0): Range {
  const walker = d.createTreeWalker(d.body, NodeFilter.SHOW_TEXT)
  let seen = 0
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const data = (node as Text).data
    for (let i = data.indexOf(word); i >= 0; i = data.indexOf(word, i + 1)) {
      if (seen++ !== n) continue
      const r = d.createRange()
      r.setStart(node, i)
      r.setEnd(node, i + word.length)
      return r
    }
  }
  throw new Error(`no ${word} #${n}`)
}

const ctx = (body: string, word: string, n = 0, options = {}) => {
  const d = doc(body)
  return selectionContext(select(d, word, n), options)!
}

describe('LK5 selection context', () => {
  it('takes the sentence of the selected occurrence, with its neighbours', () => {
    const body =
      '<p>A cache stores data. The cache is warm. Then the cache is cold.</p><p>Next one.</p>'
    for (const [n, sentence] of [
      [0, 'A cache stores data.'],
      [1, 'The cache is warm.'],
      [2, 'Then the cache is cold.'],
    ] as const) {
      expect(ctx(body, 'cache', n).sentence).toBe(sentence)
    }
    const c = ctx(body, 'cache', 1)
    expect(c.before).toBe('A cache stores data.')
    expect(c.after).toBe('Then the cache is cold.')
    expect(c.paragraph).toBe('A cache stores data. The cache is warm. Then the cache is cold.')
    expect(c.sentence.slice(c.selection.start, c.selection.end)).toBe('cache')
  })

  it('reads sentences across inline elements', () => {
    const c = ctx(
      '<p>The <em>borrow <b>checker</b></em> rejects it. It is <a href="#x">strict</a>.</p>',
      'checker',
    )
    expect(c.sentence).toBe('The borrow checker rejects it.')
    expect(c.after).toBe('It is strict.')
  })

  it('does not end a sentence at an abbreviation or an initial', () => {
    expect(ctx('<p>Dr. Smith uses a mutex, e.g. here. Done.</p>', 'mutex').sentence).toBe(
      'Dr. Smith uses a mutex, e.g. here.',
    )
    expect(ctx('<p>See Fig. 3 for the heap. Next.</p>', 'heap').sentence).toBe(
      'See Fig. 3 for the heap.',
    )
    expect(ctx('<p>As J. Doe wrote, a lock is slow. Yes.</p>', 'lock').sentence).toBe(
      'As J. Doe wrote, a lock is slow.',
    )
  })

  it('keeps quotes with their sentence', () => {
    const c = ctx('<p>He said “the stack grows down.” Then he left.</p>', 'stack')
    expect(c.sentence).toBe('He said “the stack grows down.”')
  })

  it('handles CJK and mixed text', () => {
    expect(ctx('<p>第一句。这是缓存的例子。第三句！</p>', '缓存', 0, { lang: 'zh' }).sentence).toBe(
      '这是缓存的例子。',
    )
    expect(
      ctx('<p>使用 Rust 的 borrow checker。很好。</p>', 'borrow', 0, { lang: 'zh' }).sentence,
    ).toBe('使用 Rust 的 borrow checker。')
  })

  it('stays within the cap, everything together, around the selection', () => {
    const long = Array.from({ length: 60 }, (_, i) => `Sentence ${i} is about nothing much.`).join(
      ' ',
    )
    const c = ctx(`<p>${long} The pivot word is here. ${long}</p>`, 'pivot')
    expect(c.sentence).toBe('The pivot word is here.')
    expect(c.paragraph.length).toBeLessThanOrEqual(CONTEXT_MAX)
    expect(contextLength(c)).toBeLessThanOrEqual(CONTEXT_MAX)
    expect(c.paragraph).toContain('The pivot word is here.')
    // A sentence longer than the cap is cut, keeping the selection.
    const huge = 'word '.repeat(600) + 'target ' + 'word '.repeat(600) + '.'
    const h = ctx(`<p>${huge}</p>`, 'target')
    expect(h.sentence.length).toBeLessThanOrEqual(CONTEXT_MAX)
    expect(h.sentence.slice(h.selection.start, h.selection.end)).toBe('target')
    expect(contextLength(h)).toBeLessThanOrEqual(CONTEXT_MAX)
    // A smaller cap drops the neighbours that do not fit.
    const small = ctx('<p>One two three four. The pivot. Five six seven eight.</p>', 'pivot', 0, {
      max: 20,
    })
    expect(small.sentence).toBe('The pivot.')
    expect(small.before).toBe('')
    expect(contextLength(small)).toBeLessThanOrEqual(20)
  })

  it('never leaves the chapter: the document is the chapter', () => {
    const c = ctx('<h1>Chapter 2</h1><p>First words here.</p>', 'First')
    expect(c.before).toBe('Chapter 2')
    const end = ctx('<p>Only sentence.</p>', 'Only')
    expect(end.after).toBe('')
  })

  it('continues across L16 chunks: hidden blocks are still the chapter', () => {
    const d = doc(
      Array.from({ length: 6 }, (_, i) => `<p>Block ${i} ends here.</p>`).join('') +
        '<p>The target starts this chunk.</p>',
    )
    const chunks = computeChunks(d, 80)
    showChunk(d, chunks, chunks.starts.length - 2)
    const c = selectionContext(select(d, 'target'))!
    expect(c.before).toBe('Block 5 ends here.')
  })

  it('leaves out footnote markers, note bodies and soft hyphens; keeps ligatures and code', () => {
    const c = ctx(
      '<p>The al­lo­cator frees it<a epub:type="noteref" href="#n1">1</a>. Next ﬁne one.</p>' +
        '<aside epub:type="footnote" id="n1">The note text.</aside>',
      'frees',
    )
    expect(c.sentence).toBe('The allocator frees it.')
    expect(c.after).toBe('Next ﬁne one.')
    expect(c.paragraph).not.toContain('note text')
    const code = ctx('<pre><code>let x = a-&gt;b; // the arrow</code></pre>', 'arrow')
    expect(code.sentence).toContain('a->b;')
    const aria = ctx('<p>A lock<a role="doc-noteref" href="#n">[2]</a> waits.</p>', 'waits')
    expect(aria.sentence).toBe('A lock waits.')
  })

  it('treats headings, list items and table cells as their own blocks; skips ruby text', () => {
    expect(ctx('<h2>The heap</h2><p>Memory.</p>', 'heap').sentence).toBe('The heap')
    expect(ctx('<ul><li>First item</li><li>Second item</li></ul>', 'Second').sentence).toBe(
      'Second item',
    )
    expect(ctx('<table><tr><td>Cell one</td><td>Cell two</td></tr></table>', 'two').sentence).toBe(
      'Cell two',
    )
    expect(
      ctx('<p><ruby>漢<rt>かん</rt>字<rt>じ</rt></ruby>を読む。</p>', '字', 0, { lang: 'ja' })
        .sentence,
    ).toBe('漢字を読む。')
  })

  it('skips the whitespace of indented markup between blocks', () => {
    const c = ctx(
      '<p>A bitter cold morning.</p>\n\t\t\t\n<p>Seeing the window, he went.</p>',
      'Seeing',
    )
    expect(c.before).toBe('A bitter cold morning.')
  })

  it('counts the sentences in a selection (LK9)', () => {
    expect(sentenceCount('One word')).toBe(1)
    expect(sentenceCount('One. Two.')).toBe(2)
    expect(sentenceCount('Dr. Who came.')).toBe(1)
    expect(sentenceCount('  ')).toBe(0)
  })

  it('works on plain text too, with the chapter label', () => {
    const c = contextOf('Alpha beta. Gamma delta.', 12, 17, { chapter: 'Ch 1' })
    expect(c).toMatchObject({ sentence: 'Gamma delta.', before: 'Alpha beta.', chapter: 'Ch 1' })
  })
})
