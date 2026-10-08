<script lang="ts">
  // Footnote peek (N9; Screen 17). Opens below the marker, or above it near the
  // page foot, and never covers the marker's line; it never navigates. The note is
  // copied in as safe structure (notes.ts). Esc closes and returns focus to the marker.
  import { t } from '../lib/strings'
  import { bookLang } from './bookLang'
  const lang = bookLang()
  import type { NoteEvent } from './engine'
  import { copyNote } from './notes'

  let { note, onopen, oncopy }: { note: NoteEvent; onopen: () => void; oncopy: () => void } =
    $props()

  let peek: HTMLElement | undefined = $state()
  let height = $state(0)

  const WIDTH = 440
  const GAP = 12
  const width = $derived(Math.min(WIDTH, innerWidth - 32))
  const left = $derived(Math.min(innerWidth - width - 16, Math.max(16, note.rect.left - 20)))
  /** Above the marker when the peek would run past the page foot. */
  const above = $derived(note.rect.bottom + GAP + height > innerHeight - 24)

  /** The note as safe structure only (copyNote keeps no attributes, so nothing can run). */
  const html = $derived.by(() => {
    const box = document.createElement('div')
    if (note.note) copyNote(note.note, box)
    return box.innerHTML
  })

  /** Tab from the marker moves into the peek (N9). */
  export function focusFirst() {
    peek?.querySelector<HTMLElement>('button')?.focus()
  }
</script>

<div
  class="peek"
  bind:this={peek}
  bind:clientHeight={height}
  role="dialog"
  aria-label={t.peek.label(note.label)}
  style:width="{width}px"
  style:left="{left}px"
  style:top={above ? undefined : `${note.rect.bottom + GAP}px`}
  style:bottom={above ? `${innerHeight - note.rect.top + GAP}px` : undefined}
>
  <div class="head">
    <span class="title">{t.peek.title(note.label)}</span>
    <span class="hint">{t.peek.hint}</span>
  </div>
  <!-- eslint-disable-next-line svelte/no-at-html-tags -- built by copyNote from text and bare p/em/strong/sup/sub only -->
  <div class="body" {lang}>{@html html}</div>
  {#if !note.note}
    <p class="missing">{t.peek.missing}</p>
  {/if}
  <div class="actions">
    <button type="button" class="open" onclick={onopen}>{t.peek.openInPlace}</button>
    {#if note.note}
      <button type="button" class="copy" onclick={oncopy}>{t.peek.copy}</button>
    {/if}
  </div>
</div>

<style>
  /* Screen 17: 440 px on the popover surface. */
  .peek {
    position: fixed;
    z-index: 30;
    box-sizing: border-box;
    padding: 16px 18px 14px;
    border-radius: 10px;
    background: var(--popover);
    border: 1px solid var(--popover-border);
    box-shadow: 0 12px 32px rgb(40 30 20 / 14%);
    color: var(--ink);
    font-family: var(--font-ui);
    animation: pop-in var(--motion-popover, 140ms) ease-out;
  }
  @keyframes pop-in {
    from {
      opacity: 0;
      transform: scale(0.98);
    }
  }
  .head {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    font-size: 12px;
    color: var(--ink-secondary);
  }
  .title {
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  .hint {
    font-weight: 500;
  }
  /* The note in the reading face; long notes scroll inside, up to half the window (N9). */
  .body {
    max-height: 50vh;
    overflow-y: auto;
    margin: 10px 0 12px;
    font: 18px/1.55 var(--font-reading, Literata, Georgia, serif);
  }
  .body :global(p) {
    margin: 0 0 0.5em;
  }
  .body :global(p:last-child) {
    margin-bottom: 0;
  }
  .missing {
    margin: 10px 0 12px;
    font-size: 14px;
    color: var(--ink-secondary);
  }
  .actions {
    display: flex;
    gap: 10px;
  }
  button {
    height: 36px;
    padding: 0 14px;
    border-radius: 8px;
    font: 500 13px var(--font-ui);
    cursor: default;
  }
  .open {
    border: 1px solid var(--popover-border);
    background: var(--raised);
    color: var(--ink);
  }
  .copy {
    border: 1px solid transparent;
    background: none;
    color: var(--accent);
    font-weight: 600;
  }
  .open:hover,
  .copy:hover {
    background: var(--hover-wash);
  }
</style>
