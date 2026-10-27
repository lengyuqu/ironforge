<script lang="ts">
  import LoadingState from '$lib/components/LoadingState.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import type { PullRequest } from '$lib/types/entities';
  import { createT, formatDate } from '$lib/i18n';

  const t = createT();

  let {
    owner,
    repo,
    pullRequests,
    loading,
    filter,
  }: {
    owner: string;
    repo: string;
    pullRequests: PullRequest[];
    loading: boolean;
    filter: string;
  } = $props();
</script>

{#if loading}
  <LoadingState />
{:else if pullRequests.length === 0}
  <EmptyState title={t('pulls.empty', { state: filter === 'all' ? '' : filter })} />
{:else}
  <div class="pr-list gh-list">
    {#each pullRequests as pr (pr.id)}
      <a href={`/${owner}/${repo}/pulls/${pr.number}`} class="pr-item gh-list-item">
        <span class="pr-icon">
          {pr.state === 'merged' ? '⊛' : pr.state === 'closed' ? '✓' : '⑂'}
        </span>
        <div class="pr-info">
          <div class="pr-title">
            {pr.title}
            {#if pr.is_draft}<span class="draft-badge">{t('pulls.draft')}</span>{/if}
          </div>
          <div class="pr-meta">
            #{pr.number} opened {formatDate(pr.created_at || '')} by {pr.author || t('common.unknown')}
            <span class="branch-label">{pr.head_branch}</span> → <span class="branch-label">{pr.base_branch}</span>
          </div>
        </div>
      </a>
    {/each}
  </div>
{/if}

<style>
  .pr-item {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border-light);
    text-decoration: none;
    color: var(--text-primary);
  }

  .pr-item:last-child {
    border-bottom: none;
  }

  .pr-item:hover {
    background: var(--bg-secondary);
    text-decoration: none;
  }

  .pr-icon {
    font-size: 14px;
    margin-top: 3px;
    color: var(--green);
  }

  .pr-title {
    font-weight: 600;
    font-size: 15px;
  }

  .draft-badge {
    margin-left: 6px;
    padding: 1px 6px;
    border: 1px solid var(--border);
    border-radius: 10px;
    color: var(--text-secondary);
    font-size: 11px;
  }

  .pr-meta {
    font-size: 12px;
    color: var(--text-muted);
    margin-top: 2px;
  }

  .branch-label {
    display: inline-block;
    padding: 0 6px;
    border: 1px solid var(--border);
    border-radius: 4px;
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--accent);
  }
</style>
