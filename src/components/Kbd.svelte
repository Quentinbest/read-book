<script lang="ts">
  let { keys }: { keys: string } = $props()
</script>

<kbd>{keys}</kbd>

<style>
  kbd {
    font: 500 var(--text-caption) / 1 var(--font-ui);
    color: var(--key-ink);
    letter-spacing: 0.02em;
  }
</style>
