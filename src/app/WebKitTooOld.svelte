<script lang="ts">
  // D7-WebKit: shown instead of a blank reader when this Mac's WebKit is older than
  // Safari 16.4 (provisional; styled as Screen 12's damaged-book card).
  import Modal from '../components/Modal.svelte'
  import Icon from '../components/Icon.svelte'
  import { t } from '../lib/strings/en'
  import { ipc } from './ipc'

  let { open, onclose }: { open: boolean; onclose: () => void } = $props()
</script>

<Modal label={t.library.webkitTitle} {open} {onclose} width={520}>
  <div class="card" data-webkit-too-old>
    <p class="kind"><Icon name="warning" size={18} />Safari 16.4</p>
    <h2>{t.library.webkitTitle}</h2>
    <p class="body">{t.library.webkitBody}</p>
    <div class="actions">
      <button
        type="button"
        class="primary"
        onclick={() => {
          void ipc.openSoftwareUpdate().catch(() => {})
          onclose()
        }}>{t.library.webkitUpdate}</button
      >
      <button type="button" class="plain" onclick={onclose}>{t.library.webkitClose}</button>
    </div>
  </div>
</Modal>

<style>
  .card {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 24px 32px;
    font-family: var(--font-ui);
    color: var(--ink);
  }
  .kind {
    margin: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 12px;
    font-weight: 600;
    color: var(--accent);
  }
  h2 {
    margin: 0;
    font: 500 22px/1.25 var(--font-reading, Literata, Georgia, serif);
  }
  .body {
    margin: 0;
    font-size: 14px;
    line-height: 1.5;
  }
  .actions {
    display: flex;
    gap: 12px;
    margin-top: 8px;
  }
  button {
    height: 36px;
    padding: 0 16px;
    border-radius: 8px;
    font: 600 14px var(--font-ui);
  }
  .primary {
    border: 0;
    background: var(--accent);
    color: var(--on-accent);
  }
  .plain {
    border: 0;
    background: none;
    color: var(--ink);
    font-weight: 500;
  }
  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
