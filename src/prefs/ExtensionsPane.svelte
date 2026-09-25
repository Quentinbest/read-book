<script lang="ts">
  // Settings › Extensions (Screen 11; P5, P6, P7) and the install sheet (G1,
  // provisional). Installed extensions with what they can access and what they add,
  // their switch, a quiet failure box with Restart and Disable, Remove (asking about
  // their data), pinning in the reader's ⋯ menu, and Restart without extensions.
  import { invoke } from '@tauri-apps/api/core'
  import { emit } from '@tauri-apps/api/event'
  import { open } from '@tauri-apps/plugin-dialog'
  import { onMount } from 'svelte'
  import Icon from '../components/Icon.svelte'
  import Modal from '../components/Modal.svelte'
  import { t } from '../lib/strings/en'
  import { ipc } from '../app/ipc'
  import { testHooks } from '../app/testHooks'
  import { contributionLabels, permissionLabel } from '../lib/extensions/labels'
  import type { Inspection, InstalledExtension } from '../lib/extensions/types'

  let list = $state<InstalledExtension[]>([])
  let safeMode = $state(false)
  let pinned = $state<string[]>([])
  let sheet = $state<{ path: string; inspection: Inspection } | null>(null)
  let error = $state<{ name: string; reason: string } | null>(null)
  let removing = $state<InstalledExtension | null>(null)
  let status = $state('')

  async function reload() {
    list = await invoke<InstalledExtension[]>('extensions_list')
  }
  /** The reader's window reloads its extensions. */
  async function changed() {
    await reload()
    await emit('extensions-changed')
  }

  async function pickFile() {
    // The e2e harness cannot drive the open dialog; it hands over a path instead.
    const path = testHooks?.pickExtensionFile
      ? await testHooks.pickExtensionFile()
      : await open({ filters: [{ name: 'Linen extension', extensions: ['linenext'] }] })
    if (typeof path !== 'string') return
    try {
      const inspection = await invoke<Inspection>('extension_inspect', { path })
      if (inspection.incompatible) {
        error = { name: inspection.manifest.name, reason: inspection.incompatible }
        return
      }
      sheet = { path, inspection }
    } catch (e) {
      error = {
        name: path.split('/').pop() ?? path,
        reason: String((e as { message?: string }).message ?? e),
      }
    }
  }

  async function install() {
    if (!sheet) return
    const { path, inspection } = sheet
    sheet = null
    await invoke('extension_install', { path, granted: inspection.manifest.permissions })
    status = t.extSettings.installed(inspection.manifest.name)
    await changed()
  }

  async function setEnabled(x: InstalledExtension, enabled: boolean) {
    await invoke('extension_set_enabled', { id: x.manifest.id, enabled })
    await changed()
  }

  async function restart(x: InstalledExtension) {
    await invoke('extension_restart', { id: x.manifest.id })
    await changed()
  }

  async function remove(deleteData: boolean) {
    const x = removing
    removing = null
    if (!x) return
    await invoke('extension_remove', { id: x.manifest.id, deleteData })
    pinned = pinned.filter((id) => id !== x.manifest.id)
    await ipc.settingSet('pinnedExtensions', JSON.stringify(pinned))
    await changed()
  }

  async function togglePin(x: InstalledExtension) {
    const id = x.manifest.id
    pinned = pinned.includes(id) ? pinned.filter((p) => p !== id) : [...pinned, id]
    await ipc.settingSet('pinnedExtensions', JSON.stringify(pinned))
    await emit('extensions-changed')
  }

  const kind = (x: InstalledExtension) =>
    x.builtin
      ? t.extSettings.builtin
      : x.manifest.contributes.themes.length && !x.manifest.main
        ? t.extSettings.themePack
        : null
  const recentFailures = (x: InstalledExtension) =>
    x.crashes.filter((c) => Date.now() - c < 24 * 3600 * 1000).length

  onMount(() => {
    void reload()
    void invoke<boolean>('app_safe_mode').then((v) => (safeMode = v))
    void ipc.settingGet('pinnedExtensions').then((v) => (pinned = JSON.parse(v ?? '[]')))
  })
</script>

<div class="head">
  <p class="intro">{t.prefs.extensionsIntro}</p>
  <button type="button" class="btn" onclick={() => void pickFile()}>{t.extSettings.install}</button>
</div>
{#if safeMode}<p class="banner" role="status">{t.extSettings.safeMode}</p>{/if}
{#if status}<p class="status" role="status">{status}</p>{/if}

<ul class="list">
  {#each list as x (x.manifest.id)}
    {@const m = x.manifest}
    {@const themeOnly = m.contributes.themes.length > 0 && !m.main}
    <li class="ext" data-extension={m.id}>
      <span class="icon" aria-hidden="true">{m.name.slice(0, 1)}</span>
      <div class="body">
        <div class="title-row">
          <span class="name">{m.name}</span>
          <span class="meta"
            >{[kind(x), m.version, x.enabled ? null : t.extSettings.off]
              .filter(Boolean)
              .join(' · ')}</span
          >
          <span class="grow"></span>
          <button
            type="button"
            role="switch"
            class="switch"
            aria-checked={x.enabled}
            aria-label={t.extSettings.enabled(m.name)}
            disabled={!!x.incompatible}
            onclick={() => void setEnabled(x, !x.enabled)}><span class="thumb"></span></button
          >
        </div>
        {#if m.description}<p class="description">{m.description}</p>{/if}
        {#if themeOnly}
          <p class="chips">
            <span class="label">{t.extSettings.adds}</span><span class="chip">Theme</span>
            <span class="label">{t.extSettings.can}</span><span class="plain"
              >{t.extSettings.nothing}</span
            >
          </p>
        {:else}
          <p class="chips">
            <span class="label">{t.extSettings.canAccess}</span>
            {#each m.permissions as p (p)}
              {@const l = permissionLabel(p)}
              <span class="chip" class:warn={l.warn || l.network}
                >{#if l.network}<Icon name="globe" size={12} />{/if}{l.label}</span
              >
            {:else}
              <span class="plain">{t.extSettings.noAccess}</span>
            {/each}
          </p>
          {#if contributionLabels(m).length}
            <p class="chips">
              <span class="label">{t.extSettings.adds}</span>
              {#each contributionLabels(m) as a (a)}<span class="chip">{a}</span>{/each}
            </p>
          {/if}
        {/if}
        {#if x.incompatible}
          <p class="failure">
            <Icon name="warning" size={14} />{t.extSettings.incompatible(x.incompatible)}
          </p>
        {:else if x.suspended || recentFailures(x)}
          <div class="failure" role="status">
            <Icon name="warning" size={14} />
            <span class="grow"
              >{x.suspended
                ? t.extSettings.suspended
                : t.extSettings.failures(recentFailures(x))}</span
            >
            <button type="button" class="btn small" onclick={() => void restart(x)}
              >{t.extSettings.restart}</button
            >
            <button type="button" class="btn small" onclick={() => void setEnabled(x, false)}
              >{t.extSettings.disable}</button
            >
          </div>
        {/if}
        {#if !x.builtin || m.contributes.commands.length}
          <p class="actions">
            {#if m.contributes.commands.length}
              <label class="pin"
                ><input
                  type="checkbox"
                  checked={pinned.includes(m.id)}
                  onchange={() => void togglePin(x)}
                />{t.extSettings.pin}</label
              >
            {/if}
            {#if !x.builtin}
              <button type="button" class="link" onclick={() => (removing = x)}
                >{t.extSettings.remove}</button
              >
            {/if}
          </p>
        {/if}
      </div>
    </li>
  {/each}
</ul>

<div class="foot">
  <button type="button" class="btn" onclick={() => void invoke('app_restart_safe')}
    >{t.extSettings.restartWithout}</button
  >
  <span class="help">{t.extSettings.shiftHint}</span>
</div>

<!-- G1: the install sheet (provisional). -->
<Modal
  label={sheet
    ? sheet.inspection.update_from
      ? t.extSettings.updateTitle(sheet.inspection.manifest.name, sheet.inspection.manifest.version)
      : t.extSettings.installTitle(sheet.inspection.manifest.name)
    : ''}
  open={sheet !== null}
  onclose={() => (sheet = null)}
  width={480}
>
  {#if sheet}
    {@const i = sheet.inspection}
    {@const shown = i.update_from ? i.new_permissions : i.manifest.permissions}
    <div class="sheet" data-install-sheet>
      <h2>
        {i.update_from
          ? t.extSettings.updateTitle(i.manifest.name, i.manifest.version)
          : t.extSettings.installTitle(i.manifest.name)}
      </h2>
      <p class="meta">
        {i.manifest.version}{i.manifest.publisher ? ` · ${i.manifest.publisher}` : ''}
      </p>
      {#if i.manifest.description}<p>{i.manifest.description}</p>{/if}
      <p class="unverified"><Icon name="warning" size={14} />{t.extSettings.notVerified}</p>
      <h3>{i.update_from ? t.extSettings.newAccess : t.extSettings.canAccess}</h3>
      {#if shown.length}
        <ul class="perms">
          {#each shown as p (p)}
            {@const l = permissionLabel(p)}
            <li class:warn={l.warn}>{l.label}</li>
          {/each}
        </ul>
      {:else}
        <p class="plain">{t.extSettings.noAccess}</p>
      {/if}
      {#if !i.update_from && contributionLabels(i.manifest).length}
        <h3>{t.extSettings.adds}</h3>
        <p>{contributionLabels(i.manifest).join(' · ')}</p>
      {/if}
      <div class="buttons">
        <button type="button" class="btn" onclick={() => (sheet = null)}
          >{i.update_from ? t.extSettings.keepCurrent : t.extSettings.cancel}</button
        >
        <button type="button" class="btn primary" onclick={() => void install()}
          >{i.update_from ? t.extSettings.update : t.extSettings.installButton}</button
        >
      </div>
    </div>
  {/if}
</Modal>

<Modal
  label={error ? t.extSettings.cantInstall(error.name) : ''}
  open={error !== null}
  onclose={() => (error = null)}
  width={440}
>
  {#if error}
    <div class="sheet" data-install-error>
      <h2>{t.extSettings.cantInstall(error.name)}</h2>
      <p>{error.reason}</p>
      <div class="buttons">
        <button type="button" class="btn primary" onclick={() => (error = null)}
          >{t.extSettings.close}</button
        >
      </div>
    </div>
  {/if}
</Modal>

<Modal
  label={removing ? t.extSettings.removeTitle(removing.manifest.name) : ''}
  open={removing !== null}
  onclose={() => (removing = null)}
  width={440}
>
  {#if removing}
    <div class="sheet" data-remove-sheet>
      <h2>{t.extSettings.removeTitle(removing.manifest.name)}</h2>
      <p>{t.extSettings.removeBody}</p>
      <div class="buttons">
        <button type="button" class="btn" onclick={() => (removing = null)}
          >{t.extSettings.cancel}</button
        >
        <button type="button" class="btn" onclick={() => void remove(false)}
          >{t.extSettings.keepData}</button
        >
        <button type="button" class="btn primary" onclick={() => void remove(true)}
          >{t.extSettings.deleteData}</button
        >
      </div>
    </div>
  {/if}
</Modal>

<style>
  .head {
    display: flex;
    align-items: flex-start;
    gap: 24px;
  }
  .intro {
    flex: 1;
    margin: 0;
    font-size: 13px;
    line-height: 1.5;
    color: var(--ink);
  }
  .banner,
  .status {
    margin: 0;
    font-size: 13px;
  }
  .banner {
    padding: 10px 12px;
    border-radius: 8px;
    background: var(--control-track);
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    border-top: 1px solid var(--hairline);
  }
  .ext {
    display: flex;
    gap: 16px;
    padding: 18px 0;
    border-bottom: 1px solid var(--hairline);
  }
  .icon {
    width: 40px;
    height: 40px;
    flex-shrink: 0;
    display: grid;
    place-items: center;
    border-radius: 10px;
    background: var(--control-track);
    color: var(--ink-secondary);
    font: 500 17px var(--font-reading, Literata, Georgia, serif);
  }
  .body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .title-row {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }
  .name {
    font-size: 15px;
    font-weight: 600;
  }
  .meta {
    font-size: 12px;
    color: var(--ink-secondary);
  }
  .grow {
    flex: 1;
  }
  .description,
  .chips,
  .actions {
    margin: 0;
    font-size: 13px;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  .label {
    font-size: 12px;
    font-weight: 600;
    margin-right: 4px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 3px 9px;
    border-radius: 12px;
    background: var(--control-track);
    font-size: 12px;
  }
  .chip.warn {
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--accent);
  }
  .plain {
    font-size: 12px;
    color: var(--ink-secondary);
  }
  .failure {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0;
    padding: 10px 12px;
    border-radius: 8px;
    background: color-mix(in srgb, var(--accent) 8%, var(--ground));
    font-size: 13px;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 16px;
  }
  .pin {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
  }
  .link {
    border: 0;
    padding: 0;
    background: none;
    color: var(--accent);
    font: 500 12px var(--font-ui);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: 12px;
    padding-top: 16px;
  }
  .help {
    font-size: 12px;
    color: var(--ink-secondary);
  }
  .btn {
    height: 32px;
    padding: 0 14px;
    border: 1px solid var(--popover-border);
    border-radius: 8px;
    background: var(--raised);
    color: var(--ink);
    font: 500 13px var(--font-ui);
    white-space: nowrap;
  }
  .btn.small {
    height: 28px;
  }
  .btn.primary {
    background: var(--ink);
    border-color: var(--ink);
    color: var(--ground);
    font-weight: 600;
  }
  .switch {
    width: 38px;
    height: 22px;
    padding: 2px;
    border: 0;
    border-radius: 11px;
    background: var(--control-track);
    box-shadow: inset 0 0 0 1px var(--segment-ring);
  }
  .switch .thumb {
    display: block;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--raised);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.25);
    transition: transform 120ms;
  }
  .switch[aria-checked='true'] {
    background: var(--accent);
    box-shadow: none;
  }
  .switch[aria-checked='true'] .thumb {
    transform: translateX(16px);
  }
  .btn:focus-visible,
  .switch:focus-visible,
  .link:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .sheet {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px 4px;
    font-family: var(--font-ui);
    color: var(--ink);
  }
  .sheet h2 {
    margin: 0;
    font-size: 17px;
  }
  .sheet h3 {
    margin: 8px 0 0;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--ink-secondary);
  }
  .sheet p {
    margin: 0;
    font-size: 13px;
    line-height: 1.5;
  }
  .unverified {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--ink-secondary);
  }
  .perms {
    margin: 0;
    padding-left: 18px;
    font-size: 13px;
    line-height: 1.6;
  }
  .perms .warn {
    color: var(--accent);
    font-weight: 600;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 8px;
  }
</style>
