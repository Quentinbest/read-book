<script lang="ts" generics="T extends string">
  // Tabs with automatic activation; arrow keys move between them (Navigator).
  let {
    label,
    tabs,
    selected = $bindable(),
    idPrefix,
  }: {
    label: string
    tabs: { value: T; label: string }[]
    selected: T
    idPrefix: string
  } = $props()

  let buttons: HTMLButtonElement[] = $state([])

  function onkeydown(e: KeyboardEvent, i: number) {
    let next = -1
    if (e.key === 'ArrowRight') next = (i + 1) % tabs.length
    else if (e.key === 'ArrowLeft') next = (i - 1 + tabs.length) % tabs.length
    else if (e.key === 'Home') next = 0
    else if (e.key === 'End') next = tabs.length - 1
    if (next < 0) return
    e.preventDefault()
    selected = tabs[next].value
    buttons[next]?.focus()
  }
</script>

<div class="tabs" role="tablist" aria-label={label}>
  {#each tabs as tab, i (tab.value)}
    <button
      bind:this={buttons[i]}
      type="button"
      role="tab"
      id="{idPrefix}-tab-{tab.value}"
      aria-controls="{idPrefix}-panel-{tab.value}"
      aria-selected={selected === tab.value}
      tabindex={selected === tab.value ? 0 : -1}
      onclick={() => (selected = tab.value)}
      onkeydown={(e) => onkeydown(e, i)}
    >
      {tab.label}
    </button>
  {/each}
</div>

<style>
  .tabs {
    display: grid;
    grid-auto-flow: column;
    grid-auto-columns: 1fr;
    gap: 2px;
    padding: 3px;
    border-radius: 9px;
    background: var(--hover-wash);
  }
  button {
    position: relative;
    min-height: 30px;
    border: 1px solid transparent;
    border-radius: var(--radius-control);
    background: none;
    font-weight: 500;
    color: var(--ink-secondary);
    cursor: default;
  }
  button::before {
    content: '';
    position: absolute;
    inset: -7px 0;
  }
  button[aria-selected='true'] {
    background: var(--ground);
    color: var(--ink);
    border-color: var(--ink-secondary);
  }
</style>
