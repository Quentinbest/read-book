<script lang="ts">
  import type { Snippet } from 'svelte'

  // Non-modal and anchored (Aa, Go to): no scrim, the only shadow in the UI (V5),
  // 140 ms fade from 98% (V8). Esc and outside clicks are handled by the reader
  // state machine, which owns the floating lane.
  let { label, x, y, children }: { label: string; x: number; y: number; children: Snippet } =
    $props()
</script>

<div class="popover" role="dialog" aria-label={label} style:left="{x}px" style:top="{y}px">
  {@render children()}
</div>

<style>
  .popover {
    position: fixed;
    z-index: 30;
    min-width: 240px;
    padding: 12px 16px;
    border-radius: var(--radius-popover);
    /* Screen 17: popovers sit on their own surface with a firmer border. */
    background: var(--popover, var(--ground));
    border: 1px solid var(--popover-border, var(--hairline));
    color: var(--ink);
    box-shadow: var(--shadow-popover);
    animation: pop-in var(--motion-popover) ease-out;
  }
  @keyframes pop-in {
    from {
      opacity: 0;
      transform: scale(0.98);
    }
  }
</style>
