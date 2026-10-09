<script lang="ts">
  // Settings › Dictionaries (Reading Lens DX1–DX3, DX11; Canvas 14; O3, item 78,
  // provisional). The reader's MDX dictionaries in the order the peek tries them,
  // each with its switch, Move up and down (the canvas's drag handle, by keyboard
  // too) and Remove. Add dictionary… picks an .mdx; its MDD files come with it. An
  // import builds a new generation, so the previous version stays in use until it
  // finishes or is cancelled. Refusals say why (DX3).
  import { emit } from '@tauri-apps/api/event'
  import { open } from '@tauri-apps/plugin-dialog'
  import { onMount } from 'svelte'
  import { ipc, type DictError, type DictionaryRow } from '../app/ipc'
  import { testHooks } from '../app/testHooks'
  import { t } from '../lib/strings'
  import { formatNumber } from '../lib/strings/format'

  let list = $state<DictionaryRow[]>([])
  let importing = $state<string | null>(null)
  let problem = $state<{ name: string; text: string } | null>(null)
  let status = $state('')

  async function reload() {
    list = await ipc.dictList()
  }
  /** Readers in other windows pick up the change at their next look-up. */
  async function changed() {
    await reload()
    await emit('dictionaries-changed')
  }
  onMount(() => void reload())

  function reason(e: DictError): string {
    switch (e.kind) {
      case 'alreadyInstalled':
        return t.dictSettings.alreadyInstalled(e.title)
      case 'registered':
        return t.dictSettings.registered
      case 'lzo':
        return t.dictSettings.lzo
      case 'encoding':
        return t.dictSettings.encoding(e.name)
      case 'damaged':
        return t.dictSettings.damaged
      case 'tooLarge':
        return t.dictSettings.tooLarge
      case 'changed':
        return t.dictSettings.changed
      case 'notMdx':
        return t.dictSettings.notMdx
      default:
        return t.dictSettings.failed
    }
  }

  async function add() {
    // The e2e harness cannot drive the open dialog; it hands over a path instead.
    const path = testHooks?.pickDictionaryFile
      ? await testHooks.pickDictionaryFile()
      : await open({ filters: [{ name: t.dictSettings.fileFilter, extensions: ['mdx'] }] })
    if (typeof path !== 'string') return
    const name = path.split('/').pop() ?? path
    importing = name
    problem = null
    status = ''
    try {
      const row = await ipc.dictImport(path)
      status = t.dictSettings.added(row.title)
      await changed()
    } catch (e) {
      const err = e as DictError
      if (err?.kind !== 'cancelled') problem = { name, text: reason(err) }
    } finally {
      importing = null
    }
  }

  async function move(i: number, by: number) {
    const ids = list.map((d) => d.id)
    const j = i + by
    if (j < 0 || j >= ids.length) return
    ;[ids[i], ids[j]] = [ids[j], ids[i]]
    await ipc.dictReorder(ids)
    await changed()
  }
</script>

<p class="intro">{t.dictSettings.intro}</p>

{#if !list.length && !importing && !problem}
  <p class="empty">{t.dictSettings.none}</p>
{/if}

<ul class="rows" aria-label={t.prefs.sections.dictionaries}>
  {#each list as d, i (d.id)}
    <li class="row" data-dictionary={d.name}>
      <span class="order">
        <button
          type="button"
          class="arrow"
          aria-label={t.dictSettings.moveUp(d.title)}
          disabled={i === 0}
          onclick={() => void move(i, -1)}>↑</button
        >
        <button
          type="button"
          class="arrow"
          aria-label={t.dictSettings.moveDown(d.title)}
          disabled={i === list.length - 1}
          onclick={() => void move(i, 1)}>↓</button
        >
      </span>
      <span class="what">
        <span class="title">{d.title}</span>
        <span class="meta">{t.dictSettings.details(formatNumber(d.entries), d.resources > 0)}</span>
      </span>
      <span class="actions">
        <label>
          <input
            type="checkbox"
            checked={d.enabled}
            onchange={async (e) => {
              await ipc.dictEnable(d.id, e.currentTarget.checked)
              await changed()
            }}
          />{t.dictSettings.on}
        </label>
        <button
          type="button"
          class="btn"
          aria-label={t.dictSettings.removeNamed(d.title)}
          onclick={async () => {
            await ipc.dictRemove(d.id)
            await changed()
          }}>{t.dictSettings.remove}</button
        >
      </span>
    </li>
  {/each}
  {#if importing}
    <li class="row" data-importing>
      <span class="order"></span>
      <span class="what">
        <span class="title">{importing}</span>
        <span class="meta">{t.dictSettings.importing}</span>
        <span
          class="progress"
          role="progressbar"
          aria-label={t.dictSettings.importingNamed(importing)}
        ></span>
      </span>
      <button type="button" class="btn" onclick={() => void ipc.dictCancelImport()}
        >{t.dictSettings.cancel}</button
      >
    </li>
  {/if}
  {#if problem}
    <li class="row" data-problem>
      <span class="order"></span>
      <span class="what">
        <span class="title">{problem.name}</span>
        <span class="meta error" role="alert">{problem.text}</span>
      </span>
      <button type="button" class="btn" onclick={() => (problem = null)}
        >{t.dictSettings.dismiss}</button
      >
    </li>
  {/if}
</ul>

<div class="foot">
  <button type="button" class="btn" disabled={importing !== null} onclick={() => void add()}
    >{t.dictSettings.add}</button
  >
  <span class="meta">{t.dictSettings.order}</span>
</div>
{#if status}<p class="status" role="status">{status}</p>{/if}

<style>
  .intro {
    margin: 0 0 12px;
    max-width: 620px;
    color: var(--ink-secondary);
  }
  .empty {
    margin: 0 0 8px;
    color: var(--ink-secondary);
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .row {
    display: grid;
    grid-template-columns: 22px minmax(0, 1fr) auto;
    gap: 12px;
    align-items: center;
    padding: 12px 16px;
    border: 1px solid var(--hairline);
    border-radius: 10px;
    background: var(--raised);
  }
  .order {
    display: flex;
    flex-direction: column;
  }
  .arrow {
    width: 22px;
    height: 16px;
    padding: 0;
    border: 0;
    background: none;
    color: var(--ink-secondary);
    font-size: 11px;
    line-height: 1;
  }
  .arrow:disabled {
    opacity: 0.35;
  }
  .what {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .title {
    font-weight: 600;
    font-size: 13.5px;
    overflow-wrap: anywhere;
  }
  .meta {
    font-size: 12px;
    color: var(--ink-secondary);
  }
  .error {
    color: var(--ink);
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  label {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  input[type='checkbox'] {
    width: 16px;
    height: 16px;
    margin: 0;
    accent-color: var(--accent);
  }
  .progress {
    display: block;
    width: 280px;
    max-width: 100%;
    height: 4px;
    margin-top: 4px;
    border-radius: 2px;
    background: linear-gradient(90deg, var(--accent) 0 40%, var(--control-track) 40% 100%);
  }
  .btn {
    height: 28px;
    padding: 0 12px;
    border-radius: 6px;
    border: 1px solid var(--popover-border);
    background: var(--popover);
    color: var(--ink);
    font: 500 12.5px var(--font-ui);
  }
  .btn:disabled {
    opacity: 0.5;
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 14px;
  }
  .status {
    margin: 10px 0 0;
    font-size: 12px;
    color: var(--ink-secondary);
  }
  button:focus-visible,
  input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
