<script lang="ts">
  import type { Issue } from '$lib/types/entities';
  import { createT, formatDate } from '$lib/i18n';

  const t = createT();

  let { issue }: { issue: Issue } = $props();
</script>

<div class="issue-header">
  <div class="issue-title-row">
    <h1>{issue.title}</h1>
    <span class="issue-number">#{issue.number}</span>
  </div>
  <div class="issue-meta">
    <span class="state-badge" class:open={issue.state === 'open'} class:closed={issue.state === 'closed'}>
      {t(`issues.state.${issue.state}`)}
    </span>
    <span class="text-secondary">
      {t('issues.opened_by', { date: formatDate(issue.created_at || ''), author: issue.author || t('common.unknown') })}
    </span>
    {#if issue.labels?.length}
      {#each issue.labels as label (label)}
        <span class="label-badge">{label}</span>
      {/each}
    {/if}
  </div>
</div>

<style>
  .issue-header {
    margin-bottom: 24px;
  }

  .issue-title-row {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  h1 {
    font-size: 24px;
  }

  .issue-number {
    color: var(--text-muted);
    font-size: 18px;
  }

  .issue-meta {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
    font-size: 13px;
    flex-wrap: wrap;
  }

  .state-badge {
    padding: 2px 10px;
    border-radius: 12px;
    font-size: 12px;
    font-weight: 600;
  }

  .state-badge.open { background: rgba(63, 185, 80, 0.15); color: var(--green); }
  .state-badge.closed { background: rgba(248, 81, 73, 0.15); color: var(--red); }

  .label-badge {
    display: inline-block;
    padding: 0 6px;
    border: 1px solid var(--purple);
    color: var(--purple);
    border-radius: 10px;
    font-size: 11px;
  }
</style>
