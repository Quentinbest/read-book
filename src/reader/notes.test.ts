// @vitest-environment jsdom
import { describe, expect, it } from 'vitest'
import { copyNote, isNoteRef, noteContainer, noteText, referencedFootnoteAsides } from './notes'

const xhtml = (body: string) =>
  new DOMParser().parseFromString(
    `<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body>${body}</body></html>`,
    'application/xhtml+xml',
  )

describe('N9 notes', () => {
  const doc = xhtml(`
    <p>Text<a id="r1" epub:type="noteref" href="#n1">1</a> and
       <a role="doc-noteref" href="#n2">2</a> and <a href="#plain">x</a>.</p>
    <aside id="n1" epub:type="footnote"><p>The <i>first</i> note <a epub:type="backlink" href="#r1">↩</a></p></aside>
    <aside id="n2" role="doc-footnote"><p>Second<script>alert(1)</script></p></aside>
    <aside id="other" epub:type="footnote"><p>Unreferenced</p></aside>
    <ol><li id="e1"><p><a id="t1"/>Endnote <b onclick="x()">bold</b> <img src="a.png"/></p></li></ol>`)

  it('recognises note references by EPUB and ARIA semantics', () => {
    const [a, b, c] = Array.from(doc.querySelectorAll('a'))
    expect(isNoteRef(a)).toBe(true)
    expect(isNoteRef(b)).toBe(true)
    expect(isNoteRef(c)).toBe(false)
  })

  it('hides only footnote asides that are referenced from the text', () => {
    expect(referencedFootnoteAsides(doc).map((e) => e.id)).toEqual(['n1', 'n2'])
  })

  it('finds the note around a target, also inside endnote lists', () => {
    expect(noteContainer(doc.getElementById('n1')!).id).toBe('n1')
    expect(noteContainer(doc.getElementById('t1')!).localName).toBe('li')
  })

  it('copies notes as safe structure only (§7)', () => {
    const box = document.createElement('div')
    copyNote(doc.getElementById('n1')!, box)
    expect(box.innerHTML).toBe('<p>The <em>first</em> note </p>')
    const box2 = document.createElement('div')
    copyNote(doc.getElementById('n2')!, box2)
    expect(box2.innerHTML).toBe('<p>Second</p>')
    const box3 = document.createElement('div')
    copyNote(noteContainer(doc.getElementById('t1')!), box3)
    expect(box3.innerHTML).toBe('<p>Endnote <strong>bold</strong> </p>')
    expect(box3.querySelector('[onclick], img, script')).toBeNull()
  })

  it('gives plain text for Copy', () => {
    expect(noteText(doc.getElementById('n1')!)).toBe('The first note')
  })
})
