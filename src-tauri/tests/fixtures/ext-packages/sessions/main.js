/* global linen */
// 1.1 reading sessions test: keeps the last session it heard, and how many.
'use strict'
let count = 0
linen.reading.on('sessionEnded', (s) => {
  count++
  void linen.storage.set('last', JSON.stringify({ count, ...s }))
})
