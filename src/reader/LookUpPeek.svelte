<script lang="ts">
  // Dictionary peek (1.1, approved 2026-10-01; plan §1.3 “dictionary peek providers”).
  // The definition comes from the dictionaries installed on this Mac, so nothing
  // leaves it. It sits below the selection, or above it near the page foot, like
  // the footnote peek (N9), and never navigates. Esc closes it.
  import { t } from '../lib/strings'

  let {
    word,
    definition,
    rect,
    onopen,
    onsearch,
  }: {
    word: string
    /** Undefined while looking up; null when no dictionary knows the word. */
    definition: string | null | undefined
    rect: DOMRect
    onopen: () => void
    onsearch: () => void
  } = $props()

  let peek: HTMLElement | undefined = $state()
  let height = $state(0)
  const WIDTH = 440
  const GAP = 12
  const width = $derived(Math.min(WIDTH, innerWidth - 32))
  const left = $derived(Math.min(innerWidth - width - 16, Math.max(16, rect.left - 20)))
  const above = $derived(rect.bottom + GAP + height > innerHeight - 24)

  /** The system's plain text runs senses together; numbered senses start their own line. */
  const paragraphs = $derived(
    (definition ?? '')
      .replace(/\s+(?=\d{1,2} (?=[a-z(]))/g, '\n')
      .split('\n')
      .map((p) => p.trim())
      .filter(Boolean),
  )

  export function focusFirst() {
    peek?.querySelector<HTMLElement>('button')?.focus()
  }
</script>

<div
  class="peek"
  bind:this={peek}
  bind:clientHeight={height}
  role="dialog"
  aria-label={t.lookUp.label(word)}
  data-lookup
  style:width="{width}px"
  style:left="{left}px"
  style:top={above ? undefined : `${rect.bottom + GAP}px`}
  style:bottom={above ? `${innerHeight - rect.top + GAP}px` : undefined}
>
  <div class="head">
    <span class="title">{t.lookUp.title}</span>
    <span class="hint">{t.lookUp.hint}</span>
  </div>
  {#if definition === undefined}
    <p class="missing" aria-live="polite">{t.lookUp.looking}</p>
  {:else if definition === null}
    <p class="missing" aria-live="polite">{t.lookUp.none(word)}</p>
  {:else}
    <div class="body" aria-live="polite">
      {#each paragraphs as p, i (i)}<p>{p}</p>{/each}
    </div>
  {/if}
  <div class="actions">
    <button type="button" class="open" onclick={onopen}>{t.lookUp.openDictionary}</button>
    <button type="button" class="link" onclick={onsearch}>{t.lookUp.searchBook}</button>
  </div>
</div>

<style>
  /* As the footnote peek (Screen 17): 440 px on the popover surface. */
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
  .body {
    max-height: 40vh;
    overflow-y: auto;
    margin: 10px 0 12px;
    font: 16px/1.5 var(--font-reading, Literata, Georgia, serif);
  }
  .body p {
    margin: 0 0 0.4em;
  }
  .body p:first-child {
    font-weight: 600;
  }
  .body p:last-child {
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
  .link {
    border: 1px solid transparent;
    background: none;
    color: var(--accent);
    font-weight: 600;
  }
  .open:hover,
  .link:hover {
    background: var(--hover-wash);
  }
  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  @media (prefers-reduced-motion: reduce) {
    .peek {
      animation: none;
    }
  }
</style>
