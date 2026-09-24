<script lang="ts">
  let {
    checked = $bindable(false),
    label,
    description,
  }: { checked?: boolean; label: string; description?: string } = $props()
  const id = $props.id()
</script>

<div class="row">
  <span class="text">
    <span id="{id}-label">{label}</span>
    {#if description}<span class="description" id="{id}-desc">{description}</span>{/if}
  </span>
  <button
    type="button"
    role="switch"
    aria-checked={checked}
    aria-labelledby="{id}-label"
    aria-describedby={description ? `${id}-desc` : undefined}
    class="switch"
    onclick={() => (checked = !checked)}
  >
    <span class="thumb"></span>
  </button>
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: var(--hit);
  }
  .text {
    display: flex;
    flex-direction: column;
    flex: 1;
  }
  .description {
    font-size: var(--text-caption);
    color: var(--ink-secondary);
  }
  .switch {
    position: relative;
    width: 36px;
    height: 22px;
    padding: 0;
    border-radius: 11px;
    /* V7: switches carry a ≥ 3:1 border in every state. */
    border: 1px solid var(--ink-secondary);
    background: var(--ground);
  }
  .switch[aria-checked='true'] {
    background: var(--accent);
    border-color: var(--accent);
  }
  .thumb {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--ink-secondary);
    transition: transform var(--motion-popover);
  }
  .switch[aria-checked='true'] .thumb {
    transform: translateX(14px);
    background: var(--ground);
  }
  /* Enlarge the hit area to 44 px without changing the look (X4). */
  .switch::before {
    content: '';
    position: absolute;
    inset: -11px -4px;
  }
</style>
