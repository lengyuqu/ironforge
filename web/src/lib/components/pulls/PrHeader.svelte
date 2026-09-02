<script lang="ts">
  import type { PullRequest } from '$lib/types/entities';
  import { createT, formatDate } from '$lib/i18n';

  const t = createT();

  let {
    pr,
    updatingDraft,
    onToggleDraft,
  }: {
    pr: PullRequest;
    updatingDraft: boolean;
    onToggleDraft: () => void;
  } = $props();
</script>

<div class="pr-header">
  <h1>{pr.title}</h1>
  <div class="pr-meta">
    <span class="state-badge" class:open={pr.state === 'open'} class:closed={pr.state === 'closed'} class:merged={pr.state === 'merged'}>
      {t(`pulls.state.${pr.state}`)}
    </span>
    {#if pr.is_draft}<span class="draft-badge">{t('pulls.draft')}</span>{/if}
    <span class="text-secondary">
      opened {formatDate(pr.created_at || '')} by <strong>{pr.author || t('common.unknown')}</strong>
    </span>
    <span class="branch-pair">
      <span class="branch-label">{pr.head_branch}</span>
      →
      <span class="branch-label">{pr.base_branch}</span>
    </span>
    {#if pr.state === 'open'}
      <button class="btn-link" onclick={onToggleDraft} disabled={updatingDraft}>
        {pr.is_draft ? t('pulls.mark_ready') : t('pulls.convert_draft')}
      </button>
    {/if}
  </div>
</div>

{#if pr.body}
  <div class="pr-body">
    <div class="comment-header">
      <strong>{pr.author || t('common.unknown')}</strong> commented
    </div>
    <div class="comment-body">{pr.body}</div>
  </div>
{/if}

<style>
  h1 {
    font-size: 24px;
  }

  .pr-meta {
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
  .state-badge.merged { background: rgba(188, 140, 255, 0.15); color: var(--purple); }

  .draft-badge {
    padding: 2px 8px;
    border: 1px solid var(--border);
    border-radius: 12px;
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 600;
  }

  .btn-link {
    padding: 0;
    border: none;
    background: none;
    color: var(--accent);
    cursor: pointer;
  }

  .branch-pair {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .branch-label {
    padding: 2px 8px;
    border: 1px solid var(--border);
    border-radius: 4px;
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--accent);
  }

  .pr-body {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
    margin-bottom: 12px;
  }

  .comment-header {
    padding: 8px 16px;
    background: var(--bg-tertiary);
    font-size: 13px;
    color: var(--text-secondary);
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .comment-body {
    padding: 16px;
    font-size: 14px;
    line-height: 1.6;
    white-space: pre-wrap;
  }
</style>
