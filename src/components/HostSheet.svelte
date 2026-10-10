<script lang="ts">
  // Reading Lens LK6 (Canvas 13): Linen's own sheet for a host an extension asks for.
  // Linen draws it and words it; the extension's purpose is quoted as its words.
  // Esc or Don't allow refuses; nothing is sent either way.
  import Modal from './Modal.svelte'
  import { t } from '../lib/strings'
  import type { HostRequest } from '../lib/extensions/access'

  let { request, onanswer }: { request: HostRequest | null; onanswer: (allowed: boolean) => void } =
    $props()
</script>

<Modal
  label={request ? t.extAccess.hostTitle(request.name, request.host) : ''}
  open={request !== null}
  onclose={() => onanswer(false)}
  width={470}
>
  {#if request}
    <div class="sheet" data-host-sheet={request.host}>
      <h2>{t.extAccess.hostTitle(request.name, request.host)}</h2>
      <p>{t.extAccess.hostBody(request.name)}</p>
      {#if request.purpose}
        <p class="purpose">
          <span>{t.extAccess.hostPurpose(request.name)}</span>
          {request.purpose}
        </p>
      {/if}
      <p class="note">{t.extAccess.hostNote}</p>
      <div class="buttons">
        <button type="button" class="btn" onclick={() => onanswer(false)}
          >{t.extAccess.dontAllow}</button
        >
        <button type="button" class="btn primary" onclick={() => onanswer(true)}
          >{t.extAccess.allow}</button
        >
      </div>
    </div>
  {/if}
</Modal>

<style>
  .sheet {
    padding: 20px 22px 18px;
    color: var(--ink);
    font: 13px/1.5 var(--font-ui);
  }
  h2 {
    margin: 0 0 10px;
    font-size: 15px;
    font-weight: 600;
    line-height: 1.35;
  }
  p {
    margin: 0;
  }
  .purpose {
    margin-top: 8px;
    padding-left: 10px;
    border-left: 2px solid var(--hairline);
  }
  .purpose span {
    color: var(--ink-secondary);
  }
  .note {
    margin-top: 8px;
    font-size: 12px;
    color: var(--ink-secondary);
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 16px;
  }
  .btn {
    height: 30px;
    padding: 0 14px;
    border-radius: 6px;
    border: 1px solid var(--popover-border);
    background: var(--popover);
    color: var(--ink);
    font: 500 13px var(--font-ui);
  }
  .btn.primary {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--popover);
    font-weight: 600;
  }
  .btn:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
