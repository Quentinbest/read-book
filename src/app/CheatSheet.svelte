<script lang="ts">
  // The `?` cheat sheet (K9; G10, PROVISIONAL until approved): every shortcut that
  // works here, grouped, in the palette's type and key-cap style.
  import Modal from '../components/Modal.svelte'
  import { cheatSheet } from '../lib/commands/cheatsheet'
  import type { CommandRegistry } from '../lib/commands/registry'
  import { t } from '../lib/strings'
  import { layoutLabels } from './keyLabels'

  let {
    open,
    registry,
    onclose,
  }: { open: boolean; registry: CommandRegistry; onclose: () => void } = $props()

  const sections = $derived(open ? cheatSheet(registry.available(), layoutLabels()) : [])
</script>

<Modal label={t.cheatSheet.title} {open} {onclose} width={760} top={72}>
  <div class="sheet">
    <header>
      <h2>{t.cheatSheet.title}</h2>
      <button type="button" class="close" onclick={onclose}>
        {t.cheatSheet.close}<kbd>Esc</kbd>
      </button>
    </header>
    <div class="columns">
      {#each sections as section (section.title)}
        <section>
          <h3>{section.title}</h3>
          <dl>
            {#each section.rows as row (row.label)}
              <div class="row">
                <dt>{row.label}</dt>
                <dd>
                  {#each row.keys as key, i (key)}{#if i}<span class="or">·</span>{/if}<kbd
                      >{key}</kbd
                    >{/each}
                </dd>
              </div>
            {/each}
          </dl>
        </section>
      {/each}
    </div>
    <p class="foot">{t.cheatSheet.singleKeys}</p>
  </div>
</Modal>

<style>
  .sheet {
    max-height: calc(100vh - 140px);
    overflow-y: auto;
    padding: 20px 24px 16px;
    background: var(--popover);
    color: var(--ink);
    font-family: var(--font-ui);
  }
  :global(dialog:has(.sheet)) {
    background: var(--popover);
    border-color: var(--popover-border);
    border-radius: var(--radius-sheet);
    box-shadow: 0 20px 48px rgb(40 30 20 / 18%);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 4px;
  }
  h2 {
    margin: 0;
    font-size: 17px;
    font-weight: 600;
  }
  .close {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    height: 32px;
    padding: 0 8px;
    border: 0;
    border-radius: 8px;
    background: none;
    color: var(--ink-secondary);
    font: 500 13px var(--font-ui);
    cursor: default;
  }
  .close:hover {
    background: var(--hover-wash);
  }
  .columns {
    columns: 2;
    column-gap: 32px;
  }
  section {
    break-inside: avoid;
    padding-top: 12px;
  }
  h3 {
    margin: 0 0 4px;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--ink-secondary);
  }
  dl {
    margin: 0;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 30px;
    border-bottom: 1px solid var(--hairline);
    font-size: 14px;
  }
  dt {
    min-width: 0;
  }
  dd {
    margin: 0;
    flex: none;
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .or {
    color: var(--ink-secondary);
    font-size: 12px;
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
  .foot {
    margin: 16px 0 0;
    font-size: 12px;
    color: var(--ink-secondary);
  }
</style>
