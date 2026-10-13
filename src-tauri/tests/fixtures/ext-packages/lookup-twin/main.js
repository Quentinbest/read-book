/* global linen */
// Selection bar check SB1: a lookup titled like the test provider's, so the checks can
// see that the two are told apart. It answers at once and sends nothing.
'use strict'

linen.lookups.register('explain', async (request) => ({
  status: 'ok',
  headword: request.text,
  meaning: `The twin's meaning of “${request.text}”.`,
  source: { kind: 'ai', name: 'Twin', model: 'twin-model' },
}))
