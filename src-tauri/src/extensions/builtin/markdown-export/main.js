/* global linen */
// Markdown Export (built in; P§18): a book's highlights and notes as Markdown,
// grouped by chapter in reading order, saved where the reader chooses.
'use strict'

const COLORS = { yellow: 'Yellow', green: 'Green', blue: 'Blue', rose: 'Rose' }

/** One annotation (W3C Web Annotation, A9) as Markdown. */
function item(a) {
  const quote = (a.target.selector.find((s) => s.type === 'TextQuoteSelector') || {}).exact || ''
  const lines = quote
    .trim()
    .split(/\n+/)
    .map((l) => `> ${l.trim()}`)
  const note = a.body && a.body[0] && a.body[0].value
  const colour = COLORS[a['linen:color']] || ''
  return [...lines, '', ...(note ? [note.trim(), ''] : []), `*${colour}*`, ''].join('\n')
}

function markdown(collection) {
  const out = [`# ${collection.label}`, '', `${collection.total} highlights`, '']
  let chapter = null
  for (const a of collection.first.items) {
    if (a['linen:chapter'] !== chapter) {
      chapter = a['linen:chapter']
      if (chapter) out.push(`## ${chapter}`, '')
    }
    out.push(item(a))
  }
  return (
    out
      .join('\n')
      .replace(/\n{3,}/g, '\n\n')
      .trim() + '\n'
  )
}

linen.commands.register('export-markdown', async () => {
  const collection = await linen.annotations.list()
  const text = markdown(collection)
  const name = `${collection.label.replace(/[/\\:]/g, '-')} — highlights.md`
  return linen.files.save({ suggestedName: name, content: text, type: 'text/markdown' })
})

// For the golden-file test: the same conversion, callable directly.
self.markdownExport = { markdown }
