<script lang="ts">
  // Test-only harness for Dropdown: trigger and menu snippets.
  import Dropdown from './Dropdown.svelte';

  let { open = false, placement = 'right' }: { open?: boolean; placement?: 'left' | 'right' } =
    $props();
</script>

<Dropdown {open} {placement} ariaLabel="Actions">
  {#snippet trigger(toggle)}
    <span>Trigger</span>
  {/snippet}
  {#snippet menu(close)}
    <button onclick={close}>Option A</button>
  {/snippet}
</Dropdown>
