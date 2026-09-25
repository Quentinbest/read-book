/* global linenUi */
// The Definitions tab: renders what the Worker looked up (text only, no HTML from the network).
'use strict'

const out = document.getElementById('out')

function el(tag, text, cls) {
  const e = document.createElement(tag)
  if (text) e.textContent = text
  if (cls) e.className = cls
  return e
}

function render(r) {
  if (!r) return
  out.replaceChildren()
  out.append(el('h1', r.word, 'reading'))
  if (r.phonetic) out.append(el('p', r.phonetic, 'muted'))
  if (r.loading) return out.append(el('p', 'Looking up…', 'muted'))
  if (r.error) return out.append(el('p', r.error, 'muted'))
  for (const m of r.meanings) {
    out.append(el('h2', m.part))
    const ol = el('ol')
    for (const d of m.definitions) ol.append(el('li', d))
    out.append(ol)
  }
}

linenUi.onMessage((d) => render(d && d.result))
linenUi.post({ hello: true })
