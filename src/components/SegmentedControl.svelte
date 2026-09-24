<script lang="ts" generics="T extends string">
  // A radio group drawn as segments (Aa: theme, spacing, layout). Arrow keys move
  // the selection; only the selected segment is in the tab order.
  let {
    label,
    options,
    value = $bindable(),
  }: { label: string; options: { value: T; label: string }[]; value: T } = $props()

  let buttons: HTMLButtonElement[] = $state([])

  function onkeydown(e: KeyboardEvent, i: number) {
    const step =
      e.key === 'ArrowRight' || e.key === 'ArrowDown'
        ? 1
        : e.key === 'ArrowLeft' || e.key === 'ArrowUp'
          ? -1
          : 0
    if (!step) return
    e.preventDefault()
    const next = (i + step + options.length) % options.length
    value = options[next].value
    buttons[next]?.focus()
  }
</script>

<div class="segmented" role="radiogroup" aria-label={label}>
  {#each options as option, i (option.value)}
    <button
      bind:this={buttons[i]}
      type="button"
      role="radio"
      aria-checked={value === option.value}
      tabindex={value === option.value ? 0 : -1}
      onclick={() => (value = option.value)}
      onkeydown={(e) => onkeydown(e, i)}
    >
      {option.label}
    </button>
  {/each}
</div>

<style>
  .segmented {
    display: inline-grid;
    grid-auto-flow: column;
    grid-auto-columns: 1fr;
    gap: 2px;
    padding: 3px;
    border-radius: 9px;
    background: var(--hover-wash);
  }
  button {
    min-height: 30px;
    padding: 0 12px;
    border: 1px solid transparent;
    border-radius: var(--radius-control);
    background: none;
    font-weight: 500;
    cursor: default;
    /* X4: 44 px tall target around a 30 px segment. */
    position: relative;
  }
  button::before {
    content: '';
    position: absolute;
    inset: -7px 0;
  }
  /* V7: the selected segment carries a ≥ 3:1 border, not only a fill. */
  button[aria-checked='true'] {
    background: var(--ground);
    border-color: var(--ink-secondary);
  }
</style>
