<script lang="ts">
  import type { Snippet } from 'svelte'
  import Kbd from './Kbd.svelte'

  let {
    variant = 'default',
    shortcut,
    onclick,
    children,
  }: {
    variant?: 'default' | 'primary'
    shortcut?: string
    onclick?: (e: MouseEvent) => void
    children: Snippet
  } = $props()
</script>

<!-- V5: buttons have no borders. -->
<button type="button" class="button {variant}" {onclick}>
  {@render children()}
  {#if shortcut}<Kbd keys={shortcut} />{/if}
</button>

<style>
  .button {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-height: 32px;
    padding: 0 12px;
    border: 0;
    border-radius: var(--radius-control);
    background: var(--hover-wash);
    font-weight: 500;
    cursor: default;
    /* X4: a 44 px target around a 32 px face. */
    margin-block: 6px;
  }
  .button:hover {
    filter: brightness(0.97);
  }
  .primary {
    background: var(--accent);
    color: var(--ground);
  }
  /* Full strength: a dimmed hint fails contrast on Sepia (axe, X1). */
  .primary :global(kbd) {
    color: inherit;
  }
</style>
