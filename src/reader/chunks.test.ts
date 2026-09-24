// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import { chunkCount, chunkOf, computeChunks, sectionFraction, showChunk } from './chunks'

function doc(paragraphs: number, chars = 100) {
  const d = new DOMParser().parseFromString(
    '<html><head></head><body><section></section></body></html>',
    'text/html',
  )
  const section = d.querySelector('section')!
  for (let i = 0; i < paragraphs; i++) {
    const p = d.createElement('p')
    p.textContent = `${i}`.padEnd(chars, '.')
    if (i % 10 === 0) {
      const b = d.createElement('blockquote')
      b.append(p)
      section.append(b)
    } else section.append(p)
  }
  return d
}

describe('L16 chunks', () => {
  it('divides outermost blocks into chunks of about the requested size', () => {
    const d = doc(100) // 100 × 100 chars
    const c = computeChunks(d, 1000)
    expect(c.blocks.length).toBe(100) // a <p> inside a <blockquote> counts once
    expect(chunkCount(c)).toBe(10)
    expect(c.chars.every((n) => n === 1000)).toBe(true)
    expect(c.starts[0]).toBe(0)
    expect(c.starts[chunkCount(c)]).toBe(100)
  })

  it('shows one chunk and hides the rest without removing anything', () => {
    const d = doc(100)
    const c = computeChunks(d, 1000)
    const before = d.body.innerHTML.replace(/ data-linen-hidden=""/g, '')
    showChunk(d, c, 3)
    const hidden = d.querySelectorAll('[data-linen-hidden]').length
    expect(hidden).toBe(90)
    expect(c.blocks[30].hasAttribute('data-linen-hidden')).toBe(false)
    expect(c.blocks[29].hasAttribute('data-linen-hidden')).toBe(true)
    // Same nodes, same order: CFIs and highlights stay valid.
    expect(d.body.innerHTML.replace(/ data-linen-hidden=""/g, '')).toBe(before)
    showChunk(d, c, 4)
    expect(c.blocks[30].hasAttribute('data-linen-hidden')).toBe(true)
    expect(c.blocks[45].hasAttribute('data-linen-hidden')).toBe(false)
    expect(d.getElementById('linen-chunk-style')).not.toBeNull()
  })

  it('hides wrappers whose blocks are all outside the chunk', () => {
    const d = new DOMParser().parseFromString('<html><body></body></html>', 'text/html')
    for (let s = 0; s < 4; s++) {
      const section = d.createElement('section')
      for (let i = 0; i < 10; i++) {
        const p = d.createElement('p')
        p.textContent = 'x'.repeat(100)
        section.append(p)
      }
      d.body.append(section)
    }
    const c = computeChunks(d, 1500) // chunks of 15 paragraphs straddle the sections
    showChunk(d, c, 1) // paragraphs 15–29: the ends of sections 1 and 2
    const hidden = Array.from(d.querySelectorAll('section')).map((s) =>
      s.hasAttribute('data-linen-hidden'),
    )
    expect(hidden).toEqual([true, false, false, true])
  })

  it('finds the chunk of a node, including text inside nested blocks', () => {
    const d = doc(100)
    const c = computeChunks(d, 1000)
    expect(chunkOf(c, c.blocks[57].firstChild)).toBe(5)
    const quoted = c.blocks[40].querySelector('p')!.firstChild // <blockquote><p>
    expect(chunkOf(c, quoted)).toBe(4)
    expect(chunkOf(c, d.querySelector('section'))).toBe(0)
  })

  it('maps a place in a chunk to a place in the section', () => {
    const c = computeChunks(doc(100), 1000)
    expect(sectionFraction(c, 0, 0)).toBe(0)
    expect(sectionFraction(c, 5, 0.5)).toBeCloseTo(0.55)
    expect(sectionFraction(c, 9, 1)).toBe(1)
  })
})
