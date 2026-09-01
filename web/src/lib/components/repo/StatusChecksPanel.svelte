<script lang="ts">
  // Status checks panel — pure presentation: the combined status banner and
  // the individual check cards.
  import { statusIcon, statusText, statusColor } from '$lib/utils/commitStatus';
  import type { CombinedCommitStatus, CommitStatus } from '$lib/types/entities';

  interface Props {
    combined: CombinedCommitStatus | null;
    statuses: CommitStatus[];
  }

  let { combined, statuses }: Props = $props();

  function formatDate(dateStr: string): string {
    const date = new Date(dateStr);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffMins = Math.floor(diffMs / 60000);
    const diffHours = Math.floor(diffMins / 60);
    const diffDays = Math.floor(diffHours / 24);

    if (diffMins < 1) return 'just now';
    if (diffMins < 60) return `${diffMins} minutes ago`;
    if (diffHours < 24) return `${diffHours} hours ago`;
    if (diffDays < 7) return `${diffDays} days ago`;

    return date.toLocaleDateString('en-US', {
      year: 'numeric',
      month: 'short',
      day: 'numeric'
    });
  }
</script>

{#if combined}
  <div class="combined-status" style="border-left-color: {statusColor(combined.state)}">
    <div class="status-icon-large">
      {statusIcon(combined.state)}
    </div>
    <div class="status-content">
      <h2 class="status-title">{statusText(combined.state)}</h2>
      <p class="status-count">{combined.total_count} checks</p>
    </div>
  </div>
{/if}

<div class="status-checks">
  <h3>Status Checks</h3>

  {#if statuses.length === 0}
    <div class="empty-state">
      <p>No status checks reported yet.</p>
    </div>
  {:else}
    <div class="status-list">
      {#each statuses as status, index (status.id)}
        <div class="status-card" class:alt-bg={index % 2 === 1}>
          <div class="status-card-icon">
            {statusIcon(status.state)}
          </div>
          <div class="status-card-content">
            <div class="status-card-header">
              <strong class="status-context">{status.context}</strong>
              <span class="status-date">{formatDate(status.created_at || '')}</span>
            </div>
            <p class="status-description">{status.description || 'No description'}</p>
            {#if status.target_url}
              <a
                href={status.target_url}
                target="_blank"
                rel="noopener noreferrer"
                class="status-details-link"
              >
                View Details →
              </a>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .combined-status {
    display: flex;
    align-items: center;
    gap: 1rem;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-left: 4px solid;
    border-radius: var(--radius);
    padding: 1.5rem;
    margin-bottom: 1.5rem;
  }

  .status-icon-large {
    font-size: 2rem;
    line-height: 1;
  }

  .status-content {
    flex: 1;
  }

  .status-title {
    font-size: 1.25rem;
    font-weight: 600;
    margin: 0 0 0.25rem 0;
    color: var(--text-primary);
  }

  .status-count {
    font-size: 0.875rem;
    color: var(--text-secondary);
    margin: 0;
  }

  .status-checks {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 1.5rem;
  }

  .status-checks h3 {
    font-size: 1.125rem;
    font-weight: 600;
    margin: 0 0 1rem 0;
    color: var(--text-primary);
  }

  .empty-state {
    text-align: center;
    padding: 2rem;
    color: var(--text-muted);
  }

  .status-list {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .status-card {
    display: flex;
    align-items: flex-start;
    gap: 1rem;
    padding: 1rem;
    border-radius: var(--radius);
    background: var(--bg-primary);
    transition: background 0.2s;
  }

  .status-card.alt-bg {
    background: var(--bg-hover);
  }

  .status-card:hover {
    background: var(--bg-hover);
  }

  .status-card-icon {
    font-size: 1.25rem;
    line-height: 1;
    flex-shrink: 0;
    margin-top: 0.125rem;
  }

  .status-card-content {
    flex: 1;
    min-width: 0;
  }

  .status-card-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    margin-bottom: 0.25rem;
  }

  .status-context {
    font-size: 0.9375rem;
    color: var(--text-primary);
  }

  .status-date {
    font-size: 0.8125rem;
    color: var(--text-muted);
    white-space: nowrap;
  }

  .status-description {
    font-size: 0.875rem;
    color: var(--text-secondary);
    margin: 0 0 0.5rem 0;
  }

  .status-details-link {
    display: inline-block;
    font-size: 0.875rem;
    color: var(--accent);
    text-decoration: none;
    transition: opacity 0.2s;
  }

  .status-details-link:hover {
    opacity: 0.8;
    text-decoration: underline;
  }

  @media (max-width: 900px) {
    .status-card-header {
      flex-direction: column;
      align-items: flex-start;
      gap: 0.25rem;
    }
  }
</style>
