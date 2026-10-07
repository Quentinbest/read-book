<script lang="ts">
  // The damaged-book card (E3; Screen 12): shown when a book with damaged chapters
  // is opened, until the reader chooses Read anyway. Show file and Remove as E7.
  import Modal from '../components/Modal.svelte'
  import Icon from '../components/Icon.svelte'
  import { t } from '../lib/strings'
  import { ipc, type Book } from './ipc'

  let {
    book,
    onread,
    onremove,
    onclose,
  }: {
    book: Book | null
    onread: (b: Book) => void
    onremove: (b: Book) => void
    onclose: () => void
  } = $props()

  let chapters = $state(0)
  let size = $state('')
  $effect(() => {
    chapters = 0
    size = ''
    const id = book?.id
    if (id)
      void ipc
        .bookInfo(id)
        .then((i) => {
          if (book?.id !== id) return
          chapters = i.chapters
          size =
            i.file_size >= 1 << 20
              ? `${(i.file_size / (1 << 20)).toFixed(1)} MB`
              : `${Math.max(1, Math.round(i.file_size / 1024))} KB`
        })
        .catch(() => {})
  })
</script>

<Modal label={t.library.damagedTitle} open={book !== null} {onclose} width={560}>
  {#if book}
    <div class="card">
      <p class="file">
        <Icon name="warning" size={18} />{size
          ? t.library.damagedFile(book.title, size)
          : book.title}
      </p>
      <h2>{t.library.damagedTitle}</h2>
      <p class="body">{t.library.damagedBody(book.damaged_items, chapters)}</p>
      <div class="actions">
        <button type="button" class="primary" onclick={() => onread(book)}
          >{t.library.readAnyway}</button
        >
        <button type="button" class="secondary" onclick={() => void ipc.bookShowFile(book.id)}
          >{t.library.showFileShort}</button
        >
        <button type="button" class="plain" onclick={() => onremove(book)}
          >{t.library.remove}</button
        >
      </div>
    </div>
  {/if}
</Modal>

<style>
  /* Screen 12: the damaged-book card. */
  .card {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 24px 32px;
    font-family: var(--font-ui);
    color: var(--ink);
  }
  .file {
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
  .secondary {
    border: 1px solid var(--popover-border);
    background: var(--raised);
    color: var(--ink);
    font-weight: 500;
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
