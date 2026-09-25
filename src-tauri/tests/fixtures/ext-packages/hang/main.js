/* global linen */
// P6 watchdog test: a command that never finishes (a busy loop), so neither its
// reply nor its heartbeat ever comes.
'use strict'
linen.commands.register('stall', () => {
  for (;;) {
    /* spin */
  }
})
