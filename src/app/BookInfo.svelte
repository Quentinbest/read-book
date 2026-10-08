<script lang="ts">
  // Book info (E10; G3, provisional): a modal sheet, read-only (Q4), with the
  // book's metadata and its EPUB accessibility metadata in plain words.
  import Modal from '../components/Modal.svelte'
  import { t } from '../lib/strings'
  import { ipc, type Book, type BookInfo } from './ipc'
  import BookCover from './BookCover.svelte'
  import { accessibilityLines } from '../lib/library/a11y'
  import { formatDate, formatNumber, languageName } from '../lib/strings/format'

  let { book, onclose }: { book: Book | null; onclose: () => void } = $props()

  let info = $state<BookInfo | null>(null)
  $effect(() => {
    info = null
    const id = book?.id
    if (id)
      void ipc
        .bookInfo(id)
        .then((i) => {
          if (book?.id === id) info = i
        })
        .catch(() => {})
  })

  const size = (bytes: number) =>
    bytes >= 1 << 20
      ? t.info.sizeMB(
          formatNumber(bytes / (1 << 20), { minimumFractionDigits: 1, maximumFractionDigits: 1 }),
        )
      : t.info.sizeKB(formatNumber(Math.max(1, Math.round(bytes / 1024))))
  const language = (tag: string | null) => (tag ? languageName(tag) : null)
  const rows = $derived(
    book
      ? ([
          [t.info.publisher, info?.publisher],
          [t.info.published, info?.published?.slice(0, 10)],
          [t.info.language, language(book.language)],
          [t.info.layout, book.layout === 'fixed' ? t.info.fixed : t.info.reflowable],
          [t.info.identifier, info?.identifier],
          [t.info.fileSize, info ? size(info.file_size) : null],
          [t.info.added, formatDate(book.added_at, { dateStyle: 'long' })],
        ].filter(([, v]) => v) as [string, string][])
      : [],
  )
  const a11y = $derived(info ? accessibilityLines(info.a11y) : [])
</script>

<Modal label={t.info.label} open={book !== null} {onclose} width={520}>
  {#if book}
    <div class="info">
      <header>
        <BookCover {book} width={72} height={108} lettering={false} />
        <div>
          <h2 lang={book.language ?? undefined}>{book.title}</h2>
          {#if book.authors.length}<p class="by">{book.authors.join(', ')}</p>{/if}
        </div>
      </header>
      <dl>
        {#each rows as [k, v] (k)}
          <dt>{k}</dt>
          <dd>{v}</dd>
        {/each}
      </dl>
      <h3>{t.info.accessibility}</h3>
      {#if a11y.length}
        <ul>
          {#each a11y as line (line)}<li>{line}</li>{/each}
        </ul>
      {:else if info}
        <p class="muted">{t.info.noAccessibility}</p>
      {/if}
      {#if info?.description}
        <h3>{t.info.description}</h3>
        <p class="description">{info.description}</p>
      {/if}
      <div class="actions">
        <button type="button" class="done" onclick={onclose}>{t.info.done}</button>
      </div>
    </div>
  {/if}
</Modal>

<style>
  .info {
    display: flex;
    flex-direction: column;
    gap: 12px;
    font-family: var(--font-ui);
    color: var(--ink);
    max-height: 70vh;
    overflow: auto;
  }
  header {
    display: flex;
    gap: 16px;
    align-items: center;
  }
  h2 {
    margin: 0;
    font: 500 20px/1.25 var(--font-reading, Literata, Georgia, serif);
  }
  .by {
    margin: 4px 0 0;
    color: var(--ink-secondary);
    font-size: 14px;
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 6px 16px;
    margin: 4px 0 0;
    font-size: 13px;
  }
  dt {
    color: var(--ink-secondary);
  }
  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  h3 {
    margin: 8px 0 0;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--ink-secondary);
  }
  ul {
    margin: 0;
    padding-left: 18px;
    font-size: 13px;
    line-height: 1.5;
  }
  .muted,
  .description {
    margin: 0;
    font-size: 13px;
    line-height: 1.5;
  }
  .muted {
    color: var(--ink-secondary);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
  }
  .done {
    height: 36px;
    padding: 0 18px;
    border: 0;
    border-radius: 8px;
    background: var(--ink);
    color: var(--ground);
    font: 600 13px var(--font-ui);
  }
  .done:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
