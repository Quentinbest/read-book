<script lang="ts">
  // Reading Lens LK8 (Canvas 12): an extension's options page, in a Linen dialog in
  // Settings. The page runs in a frame on the extension's own origin with the style
  // kit; what it may ask Linen for arrives as messages from that frame only: its own
  // storage, a host from its optionalPermissions (Linen's sheet) and a key (Linen's
  // native dialog). Nothing else is answered.
  import { invoke } from '@tauri-apps/api/core'
  import { onDestroy, onMount } from 'svelte'
  import Modal from '../components/Modal.svelte'
  import HostSheet from '../components/HostSheet.svelte'
  import { t } from '../lib/strings'
  import { themeVariables, type Theme } from '../lib/theme/tokens'
  import {
    hasHost,
    hasSecret,
    requestHost,
    requestSecret,
    type HostRequest,
  } from '../lib/extensions/access'
  import type { InstalledExtension } from '../lib/extensions/types'

  let {
    extension,
    theme,
    onclose,
    onchanged,
  }: {
    extension: InstalledExtension
    theme: Theme
    onclose: () => void
    /** The extension's grants changed (a host was allowed). */
    onchanged: () => Promise<InstalledExtension | undefined>
  } = $props()

  let frame: HTMLIFrameElement | undefined = $state()
  let current = $state<InstalledExtension | null>(null)
  const x = $derived(current ?? extension)
  let hostRequest = $state<(HostRequest & { resolve: (allowed: boolean) => void }) | null>(null)
  const ask = (r: HostRequest) =>
    new Promise<boolean>((resolve) => {
      hostRequest?.resolve(false)
      hostRequest = { ...r, resolve }
    })

  const id = $derived(x.manifest.id)
  const page = $derived(x.manifest.contributes.options?.page ?? '')

  function sendTheme() {
    frame?.contentWindow?.postMessage({ theme: themeVariables(theme), scheme: theme.scheme }, '*')
  }

  async function answer(method: string, p: Record<string, unknown>): Promise<unknown> {
    switch (method) {
      case 'storage.get':
        return invoke('extension_storage_get', { id, key: String(p.key) })
      case 'storage.set':
        return invoke('extension_storage_set', { id, key: String(p.key), value: String(p.value) })
      case 'storage.delete':
        return invoke('extension_storage_delete', { id, key: String(p.key) })
      case 'storage.keys':
        return invoke('extension_storage_keys', { id })
      case 'permissions.has':
        return hasHost(x, String(p.host ?? ''))
      case 'permissions.request': {
        const ok = await requestHost(x, String(p.host ?? ''), p.purpose, ask)
        if (ok) current = (await onchanged()) ?? x
        return ok
      }
      case 'secrets.has':
        return hasSecret(id, String(p.name ?? ''))
      case 'secrets.request':
        return requestSecret(x, String(p.name ?? ''), String(p.host ?? ''), p.label)
    }
    throw new Error(`unknown call ${method}`)
  }

  function onmessage(e: MessageEvent) {
    // Only this extension's own frame.
    if (!frame || e.source !== frame.contentWindow || !e.origin.startsWith(`linen-ext://${id}`))
      return
    const m = e.data as { linenCall?: number; method?: string; params?: Record<string, unknown> }
    if (typeof m?.linenCall !== 'number') return
    const reply = (msg: Record<string, unknown>) =>
      frame?.contentWindow?.postMessage({ linenReply: m.linenCall, ...msg }, '*')
    answer(String(m.method), m.params ?? {}).then(
      (result) => reply({ result: result ?? null }),
      (err: unknown) => reply({ error: err instanceof Error ? err.message : String(err) }),
    )
  }

  onMount(() => addEventListener('message', onmessage))
  onDestroy(() => removeEventListener('message', onmessage))
</script>

<Modal label={t.extAccess.optionsTitle(x.manifest.name)} open={true} {onclose} width={540}>
  <div class="options" data-options={id}>
    <h2>{t.extAccess.optionsTitle(x.manifest.name)}</h2>
    <iframe
      bind:this={frame}
      title={t.extAccess.optionsTitle(x.manifest.name)}
      src="linen-ext://{id}/{page}"
      sandbox="allow-scripts allow-same-origin allow-forms"
      onload={sendTheme}
    ></iframe>
    <div class="buttons">
      <button type="button" class="btn primary" onclick={onclose}>{t.extAccess.done}</button>
    </div>
  </div>
</Modal>
<HostSheet
  request={hostRequest}
  onanswer={(allowed) => {
    const r = hostRequest
    hostRequest = null
    r?.resolve(allowed)
  }}
/>

<style>
  .options {
    padding: 20px 22px 18px;
    color: var(--ink);
    font: 13px/1.45 var(--font-ui);
  }
  h2 {
    margin: 0 0 12px;
    font-size: 15px;
    font-weight: 600;
  }
  iframe {
    display: block;
    width: 100%;
    height: 320px;
    border: 0;
    background: var(--popover);
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    margin-top: 14px;
  }
  .btn {
    height: 30px;
    padding: 0 16px;
    border-radius: 6px;
    border: 1px solid var(--accent);
    background: var(--accent);
    color: var(--popover);
    font: 600 13px var(--font-ui);
  }
  .btn:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
