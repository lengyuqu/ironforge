<script lang="ts">
  // Test-only stub for RepoHeader tests — the real CloneModal contract is
  // covered by CloneModal.test.svelte.ts. Renders a marker div so tests can
  // assert the open state and forwarded props without any API/network logic.
  let {
    open = false,
    owner = '',
    repo = '',
    archiveRef = '',
  }: { open?: boolean; owner?: string; repo?: string; archiveRef?: string } = $props();
</script>

{#if open}
  <div
    data-testid="clone-modal-stub"
    data-owner={owner}
    data-repo={repo}
    data-archive-ref={archiveRef}
  >
    clone-stub
  </div>
{/if}
