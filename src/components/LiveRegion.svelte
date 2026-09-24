<script lang="ts">
  // Screen-reader announcements for messages (M1) and, later, page turns (X3).
  // Two off-screen live regions; text is cleared and re-set so repeats are read.
  let polite = $state('')
  let assertive = $state('')

  export function announce(text: string, politeness: 'polite' | 'assertive') {
    const set = (v: string) => (politeness === 'polite' ? (polite = v) : (assertive = v))
    set('')
    requestAnimationFrame(() => set(text))
  }
</script>

<div class="visually-hidden" role="status" aria-live="polite">{polite}</div>
<div class="visually-hidden" role="alert" aria-live="assertive">{assertive}</div>
