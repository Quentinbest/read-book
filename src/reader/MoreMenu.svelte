<script lang="ts">
  // The ⋯ menu in the reader's top bar (Screen 03; contents PROVISIONAL until
  // approved): every available command, grouped as in the cheat sheet, with its
  // shortcut. Arrow keys move, Return runs, Esc closes (the floating lane).
  import { chordLabel } from '../lib/commands/keys'
  import { sectionTitle } from '../lib/commands/cheatsheet'
  import type { Command, CommandRegistry, CommandSection } from '../lib/commands/registry'
  import { t } from '../lib/strings'
  import { layoutLabels } from '../app/keyLabels'

  let {
    registry,
    anchor,
    onclose,
    pinned = [],
  }: {
    registry: CommandRegistry
    anchor: DOMRect
    onclose: () => void
    /** P8: extensions the reader pinned in Settings. */
    pinned?: string[]
  } = $props()

  const ORDER: CommandSection[] = ['navigation', 'reading', 'search', 'annotation', 'view', 'app']
  const groups = $derived.by(() => {
    const all = registry.available().filter((c) => c.palette)
    // P8: extensions have no top-bar slot; a pinned extension's commands lead the ⋯ menu.
    const pinnedExt = all.filter((c) => c.extensionId && pinned.includes(c.extensionId))
    const byExtension = [...new Set(pinnedExt.map((c) => c.extensionId!))].map((id) => ({
      title: pinnedExt.find((c) => c.extensionId === id)?.extensionName ?? id,
      items: pinnedExt.filter((c) => c.extensionId === id),
    }))
    return [
      ...byExtension,
      ...ORDER.map((s) => ({
        title: sectionTitle(s),
        items: all.filter((c) => c.section === s),
      })),
    ].filter((g) => g.items.length)
  })
  let menu: HTMLElement | undefined = $state()

  const hint = (c: Command) => {
    const k = c.chord ?? c.singleKey
    return k ? chordLabel(k, layoutLabels()) : ''
  }

  function run(c: Command) {
    onclose()
    requestAnimationFrame(() => registry.run(c.id))
  }

  function onkeydown(e: KeyboardEvent) {
    const items = Array.from(
      menu?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not(:disabled)') ?? [],
    )
    const i = items.indexOf(document.activeElement as HTMLButtonElement)
    const next =
      e.key === 'ArrowDown'
        ? (i + 1) % items.length
        : e.key === 'ArrowUp'
          ? (i - 1 + items.length) % items.length
          : e.key === 'Home'
            ? 0
            : e.key === 'End'
              ? items.length - 1
              : -1
    if (next < 0) return
    e.preventDefault()
    e.stopPropagation()
    items[next]?.focus()
  }

  $effect(() => {
    requestAnimationFrame(() =>
      menu?.querySelector<HTMLButtonElement>('[role="menuitem"]')?.focus(),
    )
  })
</script>

<div
  class="more"
  bind:this={menu}
  role="menu"
  tabindex="-1"
  aria-label={t.reader.more}
  style:top="{anchor.bottom + 6}px"
  style:right="{Math.max(8, innerWidth - anchor.right)}px"
  {onkeydown}
>
  {#each groups as group, g (group.title)}
    {#if g}<div class="separator" role="separator"></div>{/if}
    <div class="group" role="presentation">{group.title}</div>
    {#each group.items as c (c.id)}
      <button
        type="button"
        role="menuitem"
        disabled={!(c.enabled?.() ?? true)}
        onclick={() => run(c)}
      >
        <span>{c.title}</span>
        {#if hint(c)}<span class="key">{hint(c)}</span>{/if}
      </button>
    {/each}
  {/each}
</div>

<style>
  .more {
    position: fixed;
    z-index: 30;
    min-width: 260px;
    max-height: calc(100vh - 80px);
    overflow-y: auto;
    padding: 6px;
    border-radius: 10px;
    background: var(--popover);
    border: 1px solid var(--popover-border);
    box-shadow: 0 12px 32px rgb(40 30 20 / 14%);
    font-family: var(--font-ui);
    animation: pop-in var(--motion-popover, 140ms) ease-out;
  }
  @keyframes pop-in {
    from {
      opacity: 0;
      transform: scale(0.98);
    }
  }
  .group {
    padding: 8px 10px 4px;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--ink-secondary);
  }
  .separator {
    height: 1px;
    margin: 6px 4px;
    background: var(--hairline);
  }
  button {
    width: 100%;
    height: 32px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
    padding: 0 10px;
    border: 0;
    border-radius: 6px;
    background: none;
    color: var(--ink);
    font: 400 14px var(--font-ui);
    text-align: left;
    cursor: default;
  }
  button:hover,
  button:focus-visible {
    background: var(--hover-wash);
    outline: none;
  }
  /* V4: disabled items at 40%, in menus only. */
  button:disabled {
    opacity: 0.4;
  }
  .key {
    color: var(--ink-secondary);
    font-size: 12px;
  }
</style>
