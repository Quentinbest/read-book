<script lang="ts">
  import Button from '../components/Button.svelte'
  import IconButton from '../components/IconButton.svelte'
  import Kbd from '../components/Kbd.svelte'
  import MessageBar from '../components/MessageBar.svelte'
  import Modal from '../components/Modal.svelte'
  import Popover from '../components/Popover.svelte'
  import SegmentedControl from '../components/SegmentedControl.svelte'
  import Sheet from '../components/Sheet.svelte'
  import Switch from '../components/Switch.svelte'
  import Tabs from '../components/Tabs.svelte'
  import { MessageQueue } from '../lib/reader/messages'

  let theme = $state<'paper' | 'sepia' | 'night'>('paper')
  let spacing = $state<'compact' | 'default' | 'loose'>('default')
  let tab = $state<'contents' | 'search' | 'notes'>('contents')
  let singleKeys = $state(true)
  let crossfade = $state(false)
  let modalOpen = $state(new URLSearchParams(location.search).has('modal'))

  const queue = new MessageQueue({ now: () => 0 })
  queue.push({ text: 'Highlight deleted', action: { label: 'Undo', shortcut: '⌘Z', run() {} } })
</script>

<main>
  <h1>Component gallery</h1>

  <section aria-labelledby="h-buttons">
    <h2 id="h-buttons">Buttons</h2>
    <div class="row">
      <IconButton icon="library" label="Library" shortcut="⌘L" />
      <IconButton icon="contents" label="Contents" shortcut="⌘T" />
      <IconButton icon="search" label="Search" shortcut="⌘F" pressed />
      <IconButton icon="notes" label="Highlights and notes" shortcut="⇧⌘A" />
      <IconButton icon="more" label="More" />
      <Button>Read anyway</Button>
      <Button variant="primary" shortcut="⌘O">Open a book…</Button>
      <span>Shortcut hint <Kbd keys="⇧⌘G" /></span>
    </div>
  </section>

  <section aria-labelledby="h-segments">
    <h2 id="h-segments">Segmented controls and tabs</h2>
    <SegmentedControl
      label="Theme"
      bind:value={theme}
      options={[
        { value: 'paper', label: 'Paper' },
        { value: 'sepia', label: 'Sepia' },
        { value: 'night', label: 'Night' },
      ]}
    />
    <SegmentedControl
      label="Line spacing"
      bind:value={spacing}
      options={[
        { value: 'compact', label: 'Compact' },
        { value: 'default', label: 'Default' },
        { value: 'loose', label: 'Loose' },
      ]}
    />
    <Tabs
      label="Navigator views"
      idPrefix="nav"
      bind:selected={tab}
      tabs={[
        { value: 'contents', label: 'Contents' },
        { value: 'search', label: 'Search' },
        { value: 'notes', label: 'Notes' },
      ]}
    />
    <div role="tabpanel" id="nav-panel-{tab}" aria-labelledby="nav-tab-{tab}">Panel: {tab}</div>
  </section>

  <section aria-labelledby="h-switches">
    <h2 id="h-switches">Switches</h2>
    <Switch
      label="Single-key shortcuts"
      description="H, N, [ and ] act when the page has focus"
      bind:checked={singleKeys}
    />
    <Switch label="Page-turn crossfade" bind:checked={crossfade} />
  </section>

  <section aria-labelledby="h-layers">
    <h2 id="h-layers">Layers</h2>
    <Button onclick={() => (modalOpen = true)}>Open modal</Button>
  </section>
</main>

<Popover label="Reading settings" x={640} y={80}>
  <p>Popover content</p>
</Popover>

<Sheet label="Note">
  <p>Sheet content</p>
</Sheet>

<Modal label="Command palette" open={modalOpen} onclose={() => (modalOpen = false)}>
  <div class="modal-body">
    <label for="palette-input">Command</label>
    <input id="palette-input" placeholder="Type a command, chapter or setting" />
  </div>
</Modal>

<MessageBar {queue} />

<style>
  main {
    max-width: 880px;
    padding: 32px;
  }
  h1 {
    font-size: var(--text-title);
  }
  h2 {
    font-size: var(--text-body);
    font-weight: 600;
    margin: 24px 0 8px;
  }
  .row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
  }
  section > :global(*) {
    margin-bottom: 8px;
  }
  .modal-body {
    display: grid;
    gap: 8px;
    padding: 16px;
  }
  input {
    min-height: 36px;
    padding: 0 10px;
    border: 1px solid var(--ink-secondary);
    border-radius: var(--radius-control);
    background: var(--ground);
  }
</style>
