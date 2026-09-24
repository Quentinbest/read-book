<script lang="ts">
  import type { Snippet } from 'svelte'

  // Bottom sheet (A6 below 1240 px): non-modal, 14 px radius (V4).
  let { label, children }: { label: string; children: Snippet } = $props()
</script>

<section class="sheet" aria-label={label}>
  {@render children()}
</section>

<style>
  .sheet {
    position: fixed;
    inset: auto 0 0 0;
    z-index: 30;
    max-height: 60vh;
    padding: 16px 24px 24px;
    border-radius: var(--radius-sheet) var(--radius-sheet) 0 0;
    background: var(--ground);
    border-top: 1px solid var(--hairline);
    box-shadow: var(--shadow-popover);
  }
</style>
