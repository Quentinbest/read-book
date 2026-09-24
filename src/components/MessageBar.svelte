<script lang="ts">
  import type { Message, MessageQueue } from '../lib/reader/messages'
  import { t } from '../lib/strings/en'
  import Kbd from './Kbd.svelte'

  // M1: one message at a time, bottom centre, non-modal. M2: pointer or focus pauses its timer.
  let { queue }: { queue: MessageQueue } = $props()

  let current: Message | null = $state(null)
  $effect(() => {
    current = queue.current
    return queue.subscribe(() => (current = queue.current))
  })
  $effect(() => {
    const timer = setInterval(() => queue.tick(), 250)
    return () => clearInterval(timer)
  })
</script>

{#if current}
  {@const m = current}
  <div
    class="message"
    role="group"
    aria-label={t.messages.region}
    onpointerenter={() => queue.setPaused('hover', true)}
    onpointerleave={() => queue.setPaused('hover', false)}
    onfocusin={() => queue.setPaused('focus', true)}
    onfocusout={() => queue.setPaused('focus', false)}
  >
    <span>{m.text}</span>
    {#if m.action}
      <button type="button" class="action" onclick={() => queue.act(m.id)}>
        {m.action.label}
        {#if m.action.shortcut}<Kbd keys={m.action.shortcut} />{/if}
      </button>
    {/if}
    {#if !m.persistent}
      <button
        type="button"
        class="dismiss"
        aria-label={t.messages.dismiss}
        onclick={() => queue.dismiss(m.id)}>×</button
      >
    {/if}
  </div>
{/if}

<style>
  .message {
    position: fixed;
    left: 50%;
    bottom: 24px;
    z-index: 40;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 4px 8px 4px 16px;
    border-radius: 22px;
    /* Messages are ink-coloured, like the selection bar (A1). */
    background: var(--ink);
    color: var(--ground);
    box-shadow: var(--shadow-popover);
    font-weight: 500;
  }
  button {
    min-height: 36px;
    padding: 0 10px;
    border: 0;
    border-radius: 18px;
    background: none;
    color: inherit;
    font-weight: 600;
    cursor: default;
  }
  button:hover {
    background: color-mix(in srgb, var(--ground) 14%, transparent);
  }
  .action :global(kbd) {
    color: inherit;
    opacity: 0.75;
    margin-left: 4px;
  }
  button:focus-visible {
    outline-color: var(--ground);
  }
</style>
