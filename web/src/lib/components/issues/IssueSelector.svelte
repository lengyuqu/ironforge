<script lang="ts">
  import type { Issue } from '$lib/types/entities';

  let {
    issues,
    loading,
    selectedId,
    onSelect,
  }: {
    issues: Issue[];
    loading: boolean;
    selectedId: number | null;
    onSelect: (issue: Issue) => void;
  } = $props();
</script>

<aside class="issue-sidebar">
  <div class="sidebar-header">
    <h3>Issues</h3>
  </div>
  {#if loading}
    <p class="sidebar-empty">Loading…</p>
  {:else if issues.length === 0}
    <p class="sidebar-empty">No open issues.</p>
  {:else}
    <nav class="issue-nav">
      {#each issues as issue (issue.id)}
        <button
          class="issue-item"
          class:active={selectedId === issue.id}
          onclick={() => onSelect(issue)}
        >
          <span class="issue-num">#{issue.number}</span>
          <span class="issue-title">{issue.title}</span>
        </button>
      {/each}
    </nav>
  {/if}
</aside>

<style>
  .issue-sidebar {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
    position: sticky;
    top: 24px;
  }

  .sidebar-header {
    padding: 10px 14px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-tertiary);
  }

  .sidebar-header h3 {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin: 0;
    color: var(--text-muted);
  }

  .sidebar-empty {
    padding: 16px;
    font-size: 13px;
    color: var(--text-muted);
  }

  .issue-nav {
    display: flex;
    flex-direction: column;
    max-height: 60vh;
    overflow-y: auto;
  }

  .issue-item {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 10px 14px;
    border: none;
    border-bottom: 1px solid var(--border-light);
    background: none;
    text-align: left;
    cursor: pointer;
  }

  .issue-item:hover {
    background: var(--bg-hover);
  }

  .issue-item.active {
    background: rgba(var(--accent-rgb, 88, 166, 255), 0.12);
  }

  .issue-num {
    font-size: 11px;
    color: var(--text-muted);
    font-weight: 600;
  }

  .issue-title {
    font-size: 13px;
    color: var(--text-primary);
    line-height: 1.3;
  }
</style>
