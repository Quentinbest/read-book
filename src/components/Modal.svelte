<script lang="ts">
  import type { Snippet } from 'svelte'

  // Modals (⌘K, dialogs) are the only layers that trap focus and suspend reader
  // input (S8). A native <dialog> gives the focus trap, inert background and Esc.
  let {
    label,
    open,
    onclose,
    children,
    width,
    top,
  }: {
    label: string
    open: boolean
    onclose: () => void
    children: Snippet
    /** Width in px (default: 560 px, or the window less 48 px). */
    width?: number
    /** Distance from the top of the window in px (default: centred). */
    top?: number
  } = $props()

  let dialog: HTMLDialogElement | undefined = $state()

  const FOCUSABLE =
    'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'

  // A native <dialog> makes the page inert but lets Tab leave the dialog when it
  // reaches the end; S8 needs focus trapped, so Tab cycles inside.
  function onkeydown(e: KeyboardEvent) {
    // Esc closes this modal only: it must not also reach the reader behind it (S2, S8).
    if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      onclose()
      return
    }
    if (e.key !== 'Tab' || !dialog) return
    const items = Array.from(dialog.querySelectorAll<HTMLElement>(FOCUSABLE))
    if (!items.length) return e.preventDefault()
    const first = items[0]
    const last = items[items.length - 1]
    const active = document.activeElement
    if (e.shiftKey && (active === first || !dialog.contains(active))) {
      e.preventDefault()
      last.focus()
    } else if (!e.shiftKey && (active === last || !dialog.contains(active))) {
      e.preventDefault()
      first.focus()
    }
  }

  $effect(() => {
    if (!dialog) return
    if (open && !dialog.open) dialog.showModal()
    if (!open && dialog.open) dialog.close()
  })
</script>

<dialog
  bind:this={dialog}
  aria-label={label}
  style:width={width ? `min(${width}px, calc(100vw - 48px))` : undefined}
  style:margin-top={top !== undefined ? `${top}px` : undefined}
  {onkeydown}
  oncancel={(e) => {
    e.preventDefault()
    onclose()
  }}
>
  {#if open}{@render children()}{/if}
</dialog>

<style>
  dialog {
    width: min(560px, calc(100vw - 48px));
    padding: 0;
    border: 1px solid var(--hairline);
    border-radius: var(--radius-popover);
    background: var(--ground);
    color: var(--ink);
    box-shadow: var(--shadow-popover);
  }
  /* V5: modals use a 20% scrim of the page colour. */
  dialog::backdrop {
    background: color-mix(in srgb, var(--ground) 20%, transparent);
  }
</style>
