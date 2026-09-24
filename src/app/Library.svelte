<script lang="ts">
  // Phase 1 library stub (plan: “the stub shows this state or a plain list”).
  // The full library (E6–E8) arrives in Phase 6.
  import Button from '../components/Button.svelte'
  import type { Book } from './ipc'

  let { books, dropActive, onopen }: { books: Book[]; dropActive: boolean; onopen: () => void } =
    $props()

  const progress = (b: Book) =>
    b.finished_at ? 'Finished' : b.fraction == null ? 'New' : `${Math.round(b.fraction * 100)}%`
</script>

<main class="library" class:drop={dropActive}>
  {#if books.length === 0}
    <!-- E9, Screen 12 -->
    <section class="empty" aria-labelledby="empty-title">
      <h1 id="empty-title">Your library is empty</h1>
      <p>Drop EPUB files here, or open one from your computer.</p>
      <Button variant="primary" shortcut="⌘O" onclick={onopen}>Open a book…</Button>
      <p class="note">Books are copied into your library and stay on this device.</p>
    </section>
  {:else}
    <header>
      <h1>Library</h1>
      <span class="count">{books.length} {books.length === 1 ? 'book' : 'books'}</span>
      <Button shortcut="⌘O" onclick={onopen}>Open…</Button>
    </header>
    <ul class="list">
      {#each books as book (book.id)}
        <li>
          <span
            class="cover"
            style:background={book.generated_cover_tint ?? 'var(--hairline)'}
            aria-hidden="true"
          ></span>
          <span class="meta">
            <span class="title">{book.title}</span>
            <span class="author">{book.authors.join(', ') || 'Unknown author'}</span>
          </span>
          {#if book.damaged_items > 0}
            <span class="damaged">{book.damaged_items} damaged</span>
          {/if}
          <span class="progress">{progress(book)}</span>
        </li>
      {/each}
    </ul>
  {/if}
</main>

<style>
  .library {
    min-height: 100vh;
    padding: 32px 48px;
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
