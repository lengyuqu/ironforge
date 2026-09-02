<script lang="ts">
  import { timeTracking } from '$lib/api/client.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import type { TimeEntry } from '$lib/types/entities';

  let {
    owner,
    repo,
    issueNumber,
    entries,
    loading,
    currentPage,
    totalPages,
    onPageChange,
    onRefresh,
  }: {
    owner: string;
    repo: string;
    issueNumber: number;
    entries: TimeEntry[];
    loading: boolean;
    currentPage: number;
    totalPages: number;
    onPageChange: (page: number) => void;
    onRefresh: () => void | Promise<void>;
  } = $props();

  let deletingId = $state<number | null>(null);
  let error = $state('');

  function fmtMinutes(m: number): string {
    if (m < 60) return `${m}m`;
    const h = Math.floor(m / 60);
    const rem = m % 60;
    return rem > 0 ? `${h}h ${rem}m` : `${h}h`;
  }

  async function handleDelete(id: number) {
    if (!confirm('Delete this time entry?')) return;
    try {
      deletingId = id;
      error = '';
      await timeTracking.delete(owner, repo, issueNumber, id);
      await onRefresh();
    } catch (e: any) {
      error = toErrorMessage(e, 'Failed to delete time entry');
    } finally {
      deletingId = null;
    }
  }
</script>

{#if error}
  <div class="error-banner">{error}</div>
{/if}

{#if loading}
  <p class="loading-text">Loading…</p>
{:else if entries.length === 0}
  <div class="empty">No time entries for this issue yet.</div>
{:else}
  <table class="entries-table">
    <thead>
      <tr>
        <th>Duration</th>
        <th>Note</th>
        <th>Logged</th>
        <th></th>
      </tr>
    </thead>
    <tbody>
      {#each entries as entry (entry.id)}
        <tr>
          <td class="dur-cell">{fmtMinutes(entry.duration_minutes)}</td>
          <td class="note-cell">{entry.description || '—'}</td>
          <td class="date-cell">{entry.created_at?.slice(0, 10) || ''}</td>
          <td class="act-cell">
            <button
              class="btn-danger btn-sm"
              onclick={() => handleDelete(entry.id)}
              disabled={deletingId === entry.id}
            >
              {deletingId === entry.id ? '…' : 'Delete'}
            </button>
          </td>
        </tr>
      {/each}
    </tbody>
  </table>

  {#if totalPages > 1}
    <div class="pagination">
      <button
        class="btn-outline"
        disabled={currentPage <= 1}
        onclick={() => onPageChange(currentPage - 1)}
      >
        Previous
      </button>
      <span>{currentPage} / {totalPages}</span>
      <button
        class="btn-outline"
        disabled={currentPage >= totalPages}
        onclick={() => onPageChange(currentPage + 1)}
      >
        Next
      </button>
    </div>
  {/if}
{/if}

<style>
  .loading-text {
    color: var(--text-secondary);
    text-align: center;
    padding: 32px;
  }

  .empty {
    padding: 40px;
    text-align: center;
    color: var(--text-secondary);
    font-size: 14px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }

  .entries-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 14px;
  }

  .entries-table th {
    text-align: left;
    padding: 6px 12px;
    border-bottom: 2px solid var(--border);
    color: var(--text-secondary);
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
  }

  .entries-table td {
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
  }

  .dur-cell {
    font-weight: 600;
    white-space: nowrap;
  }

  .note-cell {
    max-width: 300px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .date-cell {
    white-space: nowrap;
    color: var(--text-muted);
    font-size: 12px;
  }

  .act-cell {
    text-align: right;
  }

  .btn-outline {
    padding: 5px 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text-primary);
    font-size: 13px;
    cursor: pointer;
  }

  .btn-outline:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-danger {
    padding: 4px 10px;
    background: var(--red-dim);
    border: 1px solid var(--red);
    border-radius: var(--radius);
    color: #fff;
    font-size: 12px;
    cursor: pointer;
  }

  .btn-danger:hover:not(:disabled) {
    background: var(--red);
  }

  .btn-danger:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .btn-sm {
    padding: 4px 10px;
    font-size: 12px;
  }

  .pagination {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 16px;
    margin-top: 20px;
  }

  .pagination span {
    font-size: 13px;
    color: var(--text-secondary);
  }

  .error-banner {
    padding: 8px 12px;
    border-radius: var(--radius);
    margin-bottom: 12px;
    font-size: 13px;
    color: var(--red, #f85149);
    background: rgba(248, 81, 73, 0.1);
  }
</style>
