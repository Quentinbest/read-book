<script lang="ts">
  // An extension's Navigator tab (P4; G7): its page in a frame on the extension's
  // own origin (Spike G: never the app's), styled by Linen's style kit, which gets
  // the current theme's colours. The extension's Worker runs while the tab shows.
  // A stopped or suspended extension shows one quiet line with Restart (P6).
  import { onDestroy, onMount } from 'svelte'
  import { t } from '../lib/strings/en'
  import { themeVariables, type Theme } from '../lib/theme/tokens'
  import type { ExtensionHost } from '../extensions/host.svelte'

  let {
    host,
    extId,
    name,
    title,
    page,
    theme,
  }: {
    host: ExtensionHost
    extId: string
    name: string
    title: string
    page: string
    theme: Theme
  } = $props()

  let frame: HTMLIFrameElement | undefined = $state()
  const status = $derived(host.status[extId] ?? 'idle')
  const stopped = $derived(status === 'suspended' || status === 'not-responding')

  function sendTheme() {
    frame?.contentWindow?.postMessage({ theme: themeVariables(theme), scheme: theme.scheme }, '*')
  }
  $effect(() => {
    void theme
    sendTheme()
  })

  onMount(() => {
    host.visibleTabs.add(extId)
    if (!stopped) void host.activate(extId).catch(() => {})
  })
  onDestroy(() => host.visibleTabs.delete(extId))
</script>

{#if stopped}
  <p class="stopped" role="status">
    {t.extensions.stopped(name)} ·
    <button type="button" onclick={() => void host.restart(extId)}>{t.extensions.restart}</button>
  </p>
{:else}
  <iframe
    bind:this={frame}
    class="tab"
    {title}
    src="linen-ext://{extId}/{page}"
    sandbox="allow-scripts allow-same-origin"
    onload={sendTheme}
  ></iframe>
{/if}

<style>
  .tab {
    flex: 1;
    width: 100%;
    min-height: 0;
    border: 0;
    background: var(--panel);
  }
  .stopped {
    margin: 16px;
    font-size: 13px;
    color: var(--ink-secondary);
  }
  .stopped button {
    border: 0;
    padding: 0;
    background: none;
    color: var(--accent);
    font: 600 13px var(--font-ui);
  }
</style>
