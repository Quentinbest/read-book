<script lang="ts">
  import Icon from './Icon.svelte'
  import type { IconName } from './icons'

  // X4: every icon button has a label and a tooltip, and a 44 px hit area that
  // extends past the visible icon.
  let {
    icon,
    label,
    shortcut,
    pressed,
    onclick,
  }: {
    icon: IconName
    label: string
    shortcut?: string
    pressed?: boolean
    onclick?: (e: MouseEvent) => void
  } = $props()
</script>

<button
  class="icon-button"
  type="button"
  aria-label={label}
  aria-pressed={pressed}
  title={shortcut ? `${label} (${shortcut})` : label}
  {onclick}
>
  <span class="face"><Icon name={icon} /></span>
</button>

<style>
  .icon-button {
    position: relative;
    display: inline-grid;
    place-items: center;
    width: var(--hit);
    height: var(--hit);
    padding: 0;
    border: 0;
    background: none;
    color: var(--ink);
    cursor: default;
  }
  .face {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: var(--radius-control);
  }
  /* V6: hover is a 6% ink wash. */
  .icon-button:hover .face {
    background: var(--hover-wash);
  }
  .icon-button[aria-pressed='true'] .face {
    background: var(--hover-wash);
    color: var(--accent);
  }
  .icon-button:focus-visible {
    outline: none;
  }
  .icon-button:focus-visible .face {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
