<script lang="ts">
  import type { Issue } from '$lib/types/entities';
  import { createT, formatDate } from '$lib/i18n';

  const t = createT();

  let {
    owner,
    repo,
    issues,
    loading,
    filter,
  }: {
    owner: string;
    repo: string;
    issues: Issue[];
    loading: boolean;
    filter: string;
  } = $props();

  function emptyStateLabel(): string {
    if (filter === 'all') return t('common.all');
    return t(`issues.state_label.${filter}`, filter);
  }
</script>

{#if loading}
  <p class="text-secondary">{t('common.loading')}</p>
{:else if issues.length === 0}
  <div class="empty">
    <p>{t('issues.empty', { state: emptyStateLabel() })}</p>
  </div>
{:else}
  <div class="issue-list gh-list">
    {#each issues as issue (issue.id)}
      <a href={`/${owner}/${repo}/issues/${issue.number}`} class="issue-item gh-list-item">
        <span class="issue-icon">
          {issue.state === 'closed' ? '✓' : '●'}
        </span>
        <div class="issue-info">
          <div class="issue-title">{issue.title}</div>
          <div class="issue-meta">
            {t('issues.meta', {
              number: issue.number,
              date: formatDate(issue.created_at || ''),
              author: issue.author || t('common.unknown')
            })}
            {#if issue.labels?.length}
              {#each issue.labels as label (label)}
                <span class="label-badge">{label}</span>
              {/each}
            {/if}
          </div>
        </div>
      </a>
    {/each}
  </div>
{/if}

<style>
  .empty {
    text-align: center;
    padding: 48px;
    color: var(--text-secondary);
  }

  .issue-item {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border-light);
    text-decoration: none;
    color: var(--text-primary);
  }

  .issue-item:last-child {
    border-bottom: none;
  }

  .issue-item:hover {
    background: var(--bg-secondary);
    text-decoration: none;
  }

  .issue-icon {
    font-size: 14px;
    margin-top: 3px;
    color: var(--green);
  }

  .issue-title {
    font-weight: 600;
    font-size: 15px;
  }

  .issue-meta {
    font-size: 12px;
    color: var(--text-muted);
    margin-top: 2px;
  }

  .label-badge {
    display: inline-block;
    padding: 0 6px;
    border: 1px solid var(--purple);
    color: var(--purple);
    border-radius: 10px;
    font-size: 11px;
    margin-left: 4px;
  }
</style>
