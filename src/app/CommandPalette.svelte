<script lang="ts">
  // ⌘K (K9, M3, S8; Screen 16): Recently closed, Reading and the book's chapters,
  // fuzzy-matched, with shortcut hints; ↑ ↓ move, ↵ runs, ? opens the cheat sheet.
  // A modal: it suspends reader input while open (S8).
  import Icon from '../components/Icon.svelte'
  import Modal from '../components/Modal.svelte'
  import { chordLabel } from '../lib/commands/keys'
  import { fuzzy, rank } from '../lib/commands/fuzzy'
  import type { Command, CommandRegistry } from '../lib/commands/registry'
  import type { MessageQueue } from '../lib/reader/messages'
  import { t } from '../lib/strings'
  import { layoutLabels } from './keyLabels'
  import { paletteChapters } from './palette'

  let {
    open,
    registry,
    messages,
    onclose,
  }: { open: boolean; registry: CommandRegistry; messages: MessageQueue; onclose: () => void } =
    $props()

  interface Row {
    id: string
    label: string
    hint: string
    run: () => void
  }
  interface Section {
    title: string
    rows: Row[]
  }

  let query = $state('')
  let selected = $state(0)
  let input: HTMLInputElement | undefined = $state()
  let list: HTMLElement | undefined = $state()

  /** The shortcut shown beside a command: its chord, else its single key. */
  export function shortcutOf(c: Pick<Command, 'chord' | 'singleKey'>): string {
    const chord = c.chord ?? c.singleKey
    return chord ? chordLabel(chord, layoutLabels()) : ''
  }

  const sections = $derived.by((): Section[] => {
    if (!open) return []
    const closed: Row[] = messages.recentlyClosed
      .filter((m) => m.action)
      .map((m) => ({
        id: `closed:${m.id}`,
        // “Back to page 43” names its action; “Resumed in …” is named by it (“Go to beginning”).
        label:
          m.closedLabel ??
          (m.text.startsWith(m.action!.label) ? m.text : `${m.action!.label} (${m.text})`),
        hint: m.action!.shortcut ?? '',
        run: () => messages.act(m.id),
      }))
    const commands: Row[] = registry
      .available()
      .filter((c) => c.palette && (c.enabled?.() ?? true))
      .map((c) => ({
        id: c.id,
        label: c.title,
        hint: shortcutOf(c),
        run: () => registry.run(c.id),
      }))
    const q = query.trim()
    const chapters: Row[] = q
      ? paletteChapters().map((ch, i) => ({
          id: `chapter:${i}`,
          label: ch.label,
          hint: '',
          run: ch.run,
        }))
      : []
    const by = (rows: Row[]) => rank(q, rows, (r) => r.label)
    const found = [
      { title: t.palette.recentlyClosed, rows: by(closed) },
      { title: t.palette.reading, rows: by(commands) },
      { title: t.palette.chapters, rows: by(chapters).slice(0, 8) },
    ].filter((s) => s.rows.length)
    // With a query, the section holding the best match comes first, so ↵ runs it;
    // without one, Screen 16's order stands.
    if (!q) return found
    const best = (s: Section) => fuzzy(q, s.rows[0].label)?.score ?? -Infinity
    return found.sort((a, b) => best(b) - best(a))
  })
  const rows = $derived(sections.flatMap((s) => s.rows))

  $effect(() => {
    void query
    selected = 0
  })
  $effect(() => {
    if (!open) return
    query = ''
    requestAnimationFrame(() => input?.focus())
  })
  $effect(() => {
    list?.querySelector(`[data-index="${selected}"]`)?.scrollIntoView({ block: 'nearest' })
  })

  function run(row: Row | undefined) {
    if (!row) return
    onclose()
    // After the palette has closed, so the command acts on the reader, not the modal.
    requestAnimationFrame(row.run)
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      const n = rows.length || 1
      selected = (selected + (e.key === 'ArrowDown' ? 1 : -1) + n) % n
    } else if (e.key === 'Enter') {
      e.preventDefault()
      run(rows[selected])
    } else if (e.key === '?' && !query) {
      // K9: ? from ⌘K opens the cheat sheet.
      e.preventDefault()
      onclose()
      requestAnimationFrame(() => registry.run('shortcuts.show'))
    }
  }
</script>

<Modal label={t.palette.label} {open} {onclose} width={600} top={96}>
  <div class="palette">
    <div class="field">
      <Icon name="search" size={18} />
      <input
        bind:this={input}
        bind:value={query}
        {onkeydown}
        placeholder={t.palette.placeholder}
        aria-label={t.palette.placeholder}
        role="combobox"
        aria-expanded="true"
        aria-controls="palette-list"
        aria-activedescendant={rows.length ? `palette-row-${selected}` : undefined}
        autocomplete="off"
        spellcheck="false"
      />
      <kbd>Esc</kbd>
    </div>
    <div
      class="list"
      id="palette-list"
      role="listbox"
      aria-label={t.palette.label}
      bind:this={list}
    >
      {#each sections as section (section.title)}
        <div class="section" role="presentation">{section.title}</div>
        {#each section.rows as row (row.id)}
          {@const index = rows.indexOf(row)}
          <div
            class="row"
            class:selected={index === selected}
            id="palette-row-{index}"
            data-index={index}
            role="option"
            aria-selected={index === selected}
            tabindex="-1"
            onpointermove={() => (selected = index)}
            onclick={() => run(row)}
            onkeydown={() => {}}
          >
            <span class="label">{row.label}</span>
            {#if row.hint}<kbd>{row.hint}</kbd>{/if}
          </div>
        {/each}
      {:else}
        <p class="empty">{t.palette.none}</p>
      {/each}
    </div>
    <div class="foot" aria-hidden="true">
      <span><kbd>↑</kbd><kbd>↓</kbd> {t.palette.move}</span>
      <span><kbd>↵</kbd> {t.palette.run}</span>
      <span><kbd>?</kbd> {t.palette.all}</span>
    </div>
  </div>
</Modal>

<style>
  /* Screen 16: the popover surface, 600 px wide, 96 px from the top. */
  .palette {
    display: flex;
    flex-direction: column;
    max-height: min(522px, calc(100vh - 140px));
    padding-bottom: 8px;
    background: var(--popover);
    font-family: var(--font-ui);
    color: var(--ink);
  }
  :global(dialog:has(.palette)) {
    background: var(--popover);
    border-color: var(--popover-border);
    border-radius: var(--radius-sheet);
    box-shadow: 0 20px 48px rgb(40 30 20 / 18%);
  }
  .field {
    flex: none;
    height: 56px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 16px;
    border-bottom: 1px solid var(--hairline);
    color: var(--ink-secondary);
  }
  input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: none;
    background: none;
    color: var(--ink);
    font: 400 16px var(--font-ui);
  }
  input::placeholder {
    color: var(--ink-secondary);
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 6px;
  }
  .section {
    padding: 12px 10px 6px;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--ink-secondary);
  }
  .row {
    height: 40px;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 0 16px;
    border-radius: 8px;
    font-size: 14px;
    cursor: default;
  }
  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* The selected row: a 10% accent tint with a 1 px accent ring (Screen 16). */
  .row.selected {
    background: color-mix(in srgb, var(--accent) 10%, transparent);
    box-shadow: inset 0 0 0 1px var(--accent);
  }
  kbd {
    display: inline-block;
    min-width: 10px;
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--raised);
    border: 1px solid var(--popover-border);
    color: var(--key-ink);
    font: 500 12px var(--font-ui);
    text-align: center;
  }
  .empty {
    padding: 16px;
    margin: 0;
    font-size: 14px;
    color: var(--ink-secondary);
  }
  .foot {
    flex: none;
    display: flex;
    gap: 16px;
    padding: 10px 16px 4px;
    font-size: 12px;
    color: var(--ink-secondary);
    border-top: 1px solid var(--hairline);
  }
  .foot kbd {
    margin-right: 2px;
  }
</style>
