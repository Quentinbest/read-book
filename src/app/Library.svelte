<script lang="ts">
  // Phase 1 library stub (plan: “the stub shows this state or a plain list”).
  // The full library (E6–E8) arrives in Phase 6.
  import Button from '../components/Button.svelte'
  import { t } from '../lib/strings/en'
  import type { Book } from './ipc'

  let {
    books,
    dropActive,
    onopen,
    onopenbook,
  }: {
    books: Book[]
    dropActive: boolean
    onopen: () => void
    onopenbook: (book: Book) => void
  } = $props()

  const progress = (b: Book) =>
    b.finished_at
      ? t.library.finished
      : b.fraction == null
        ? t.library.new
        : t.library.percent(b.fraction)
</script>

<div class="titlebar" data-tauri-drag-region></div>
<main class="library" class:drop={dropActive}>
  {#if books.length === 0}
    <!-- E9, Screen 12 -->
    <section class="empty" aria-labelledby="empty-title">
      <h1 id="empty-title">{t.library.emptyTitle}</h1>
      <p>{t.library.emptyBody}</p>
      <Button variant="primary" shortcut="⌘O" onclick={onopen}>{t.library.openBook}</Button>
      <p class="note">{t.library.note}</p>
    </section>
  {:else}
    <header>
      <h1>{t.library.title}</h1>
      <span class="count">{t.library.count(books.length)}</span>
      <Button shortcut="⌘O" onclick={onopen}>{t.library.open}</Button>
    </header>
    <ul class="list">
      {#each books as book (book.id)}
        <li>
          <button type="button" class="row" onclick={() => onopenbook(book)}>
            <span
              class="cover"
              style:background={book.generated_cover_tint ?? 'var(--hairline)'}
              aria-hidden="true"
            ></span>
            <span class="meta">
              <span class="title">{book.title}</span>
              <span class="author">{book.authors.join(', ') || t.library.unknownAuthor}</span>
            </span>
            {#if book.damaged_items > 0}
              <span class="damaged">{t.library.damaged(book.damaged_items)}</span>
            {/if}
            <span class="progress">{progress(book)}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</main>

<style>
  /* The window's title bar is an overlay (Screens 02/03); this strip keeps it draggable. */
  .titlebar {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    height: 52px;
    z-index: 5;
  }
  .library {
    min-height: 100vh;
    padding: 64px 48px 32px;
  }
  .drop {
    outline: 2px dashed var(--accent);
    outline-offset: -12px;
  }
  .empty {
    display: grid;
    justify-items: center;
    gap: 8px;
    max-width: 420px;
    margin: 18vh auto 0;
    text-align: center;
  }
  h1 {
    margin: 0;
    font-size: var(--text-title);
    font-weight: 600;
  }
  p {
    margin: 0;
  }
  .note {
    margin-top: 8px;
    font-size: var(--text-caption);
    color: var(--ink-secondary);
  }
  header {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 16px;
  }
  .count {
    flex: 1;
    color: var(--ink-secondary);
  }
  .list {
    margin: 0;
    padding: 0;
    list-style: none;
    border-top: 1px solid var(--hairline);
  }
  li {
    display: block;
    border-bottom: 1px solid var(--hairline);
  }
  .row {
    width: 100%;
    padding: 0;
    border: 0;
    background: none;
    text-align: start;
    cursor: default;
    display: flex;
    align-items: center;
    gap: 16px;
    min-height: 56px;
    border-bottom: 1px solid var(--hairline);
  }
  .cover {
    width: 28px;
    height: 40px;
    border-radius: 2px;
  }
  .meta {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
  }
  .title {
    font-weight: 500;
  }
  .author,
  .progress,
  .damaged {
    font-size: var(--text-caption);
    color: var(--ink-secondary);
  }
</style>
