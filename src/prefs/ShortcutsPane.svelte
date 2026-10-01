<script lang="ts">
  // Settings › Shortcuts: remapping (C4; 1.1, approved 2026-10-01). Every command, grouped
  // as on the cheat sheet, with its shortcut. Change records the next chord (Esc
  // cancels); Remove takes a shortcut away; Reset goes back to Linen's. Extension
  // commands can be given a shortcut, but never one that Linen's own commands use.
  import { onMount } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import { t } from '../lib/strings/en'
  import { ipc } from '../app/ipc'
  import { changeSetting, onSettingChanged } from '../app/settingsSync'
  import { chordLabel } from '../lib/commands/keys'
  import { SECTION_TITLES } from '../lib/commands/cheatsheet'
  import { CORE_COMMANDS, type CommandDef, type CommandSection } from '../lib/commands/registry'
  import {
    OVERRIDES_SETTING,
    assign,
    chordFromEvent,
    remappable,
    effectiveChords,
    parseOverrides,
    reset,
    unassign,
    type Overrides,
  } from '../lib/commands/remap'
  import type { InstalledExtension } from '../lib/extensions/types'

  let { source }: { source: ReturnType<Crypto['randomUUID']> } = $props()

  let overrides = $state<Overrides>({})
  let extensionDefs = $state<CommandDef[]>([])
  /** The command whose new shortcut is being typed. */
  let recording = $state<string | null>(null)
  let status = $state('')

  const ORDER: CommandSection[] = [
    'navigation',
    'reading',
    'search',
    'annotation',
    'view',
    'app',
    'extension',
  ]
  const defs = $derived([...CORE_COMMANDS, ...extensionDefs])
  const groups = $derived(
    ORDER.map((s) => ({
      title: SECTION_TITLES[s],
      // Esc and ⌘, stay Linen's (K14, G2); extensions' commands are all listed.
      items: defs.filter((d) => d.section === s && (d.palette || d.chord) && remappable(d)),
    })).filter((g) => g.items.length),
  )
  const label = (d: CommandDef) => {
    const c = effectiveChords(d, overrides)
    return c.length ? c.map((x) => chordLabel(x)).join(' ') : t.shortcuts.none
  }

  async function save(next: Overrides) {
    overrides = next
    await changeSetting(OVERRIDES_SETTING, JSON.stringify(next), source)
  }

  function onkey(e: KeyboardEvent) {
    if (!recording) return
    e.preventDefault()
    e.stopImmediatePropagation()
    if (e.code === 'Escape' && !e.metaKey && !e.ctrlKey && !e.altKey) {
      recording = null
      status = ''
      return
    }
    const chord = chordFromEvent(e)
    if (!chord) return
    const id = recording
    const r = assign(defs, overrides, id, chord)
    if (!r.ok) {
      status =
        r.problem === 'core'
          ? t.shortcuts.taken(chordLabel(chord), r.holder?.title ?? '')
          : r.problem === 'reserved'
            ? t.shortcuts.reserved(chordLabel(chord))
            : t.shortcuts.needsModifier
      return
    }
    recording = null
    status = r.displaced ? t.shortcuts.moved(chordLabel(chord), r.displaced.title) : ''
    void save(r.overrides)
  }

  onMount(() => {
    void (async () => {
      overrides = parseOverrides(await ipc.settingGet(OVERRIDES_SETTING))
      const list = await invoke<InstalledExtension[]>('extensions_list').catch(() => [])
      extensionDefs = list
        .filter((x) => x.enabled && !x.incompatible)
        .flatMap((x) =>
          x.manifest.contributes.commands.map((c) => ({
            id: `extension:${x.manifest.id}:${c.id}`,
            rule: 'P1',
            title: `${c.title} (${x.manifest.name})`,
            section: 'extension' as const,
            palette: true,
            extensionId: x.manifest.id,
            extensionName: x.manifest.name,
          })),
        )
    })()
    // Capture: the recorded chord must not also run a command.
    window.addEventListener('keydown', onkey, true)
    const off = onSettingChanged(({ key, value }) => {
      if (key === OVERRIDES_SETTING) overrides = parseOverrides(value)
    }, source)
    return () => {
      window.removeEventListener('keydown', onkey, true)
      void off.then((f) => f())
    }
  })
</script>

<div class="shortcuts" data-shortcuts>
  <p class="help">{t.shortcuts.intro}</p>
  {#each groups as g (g.title)}
    <section>
      <h2>{g.title}</h2>
      <ul>
        {#each g.items as d (d.id)}
          <li data-command={d.id} class:recording={recording === d.id}>
            <span class="title">{d.title}</span>
            <span class="keys" aria-live={recording === d.id ? 'polite' : undefined}
              >{recording === d.id ? t.shortcuts.typeNow : label(d)}</span
            >
            <span class="actions">
              <button
                type="button"
                class="btn small"
                aria-label={t.shortcuts.changeFor(d.title)}
                onclick={() => {
                  recording = recording === d.id ? null : d.id
                  status = ''
                }}>{recording === d.id ? t.shortcuts.cancel : t.shortcuts.change}</button
              >
              {#if effectiveChords(d, overrides).length && recording !== d.id}
                <button
                  type="button"
                  class="btn small plain"
                  aria-label={t.shortcuts.removeFor(d.title)}
                  onclick={() => void save(unassign(overrides, d.id))}>{t.shortcuts.remove}</button
                >
              {/if}
              {#if d.id in overrides && recording !== d.id}
                <button
                  type="button"
                  class="btn small plain"
                  aria-label={t.shortcuts.resetFor(d.title)}
                  onclick={() => void save(reset(overrides, d.id))}>{t.shortcuts.reset}</button
                >
              {/if}
            </span>
          </li>
        {/each}
      </ul>
    </section>
  {/each}
  {#if status}<p class="status" role="status">{status}</p>{/if}
  <div>
    <button
      type="button"
      class="btn"
      disabled={!Object.keys(overrides).length}
      onclick={() => {
        status = ''
        void save({})
      }}>{t.shortcuts.resetAll}</button
    >
  </div>
</div>

<style>
  .shortcuts {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  h2 {
    margin: 0 0 6px;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--ink-secondary);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 110px auto;
    align-items: center;
    gap: 12px;
    min-height: 40px;
    border-bottom: 1px solid var(--hairline);
    font-size: 13px;
  }
  li.recording .keys {
    color: var(--accent);
    font-weight: 600;
  }
  .keys {
    font-variant-numeric: tabular-nums;
    color: var(--ink-secondary);
  }
  .actions {
    display: flex;
    gap: 6px;
    justify-content: flex-end;
  }
  .help,
  .status {
    margin: 0;
    font-size: 13px;
    line-height: 1.5;
  }
  .help {
    color: var(--ink-secondary);
  }
  .status {
    color: var(--ink);
  }
  .btn {
    height: 32px;
    padding: 0 14px;
    border: 1px solid var(--popover-border);
    border-radius: 8px;
    background: var(--raised);
    color: var(--ink);
    font: 500 13px var(--font-ui);
  }
  .btn.small {
    height: 26px;
    padding: 0 10px;
    font-size: 12px;
  }
  .btn.plain {
    border-color: transparent;
    background: none;
    color: var(--accent);
  }
  .btn:disabled {
    opacity: 0.5;
  }
  .btn:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
