<script lang="ts">
  import { reviews } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT, formatDate } from '$lib/i18n';
  import type { PrReview } from '$lib/types/entities';

  const t = createT();

  let {
    owner,
    repo,
    prNumber,
    reviews: reviewList,
    onDismissed,
  }: {
    owner: string;
    repo: string;
    prNumber: number;
    reviews: PrReview[];
    onDismissed: () => void | Promise<void>;
  } = $props();

  let dismissingId = $state<number | null>(null);

  const ACTION_LABELS: Record<string, { label: string; cls: string }> = {
    approve: { label: t('pulls.review.approved', 'Approved'), cls: 'approved' },
    request_changes: { label: t('pulls.review.changes_requested', 'Changes requested'), cls: 'changes' },
    comment: { label: t('pulls.review.commented', 'Commented'), cls: 'comment' },
    dismiss: { label: t('pulls.review.dismissed', 'Dismissed'), cls: 'dismissed' },
  };

  function actionInfo(action: string) {
    return ACTION_LABELS[action] || { label: action, cls: 'comment' };
  }

  async function handleDismiss(review: PrReview) {
    const message = prompt(t('pulls.review.dismiss_prompt', 'Reason for dismissing this review:'));
    if (message === null) return;
    const reason = message.trim() || t('pulls.review.dismiss_default', 'Review dismissed');

    dismissingId = review.id;
    try {
      await reviews.dismiss(owner, repo, prNumber, review.id, reason);
      toast.success(t('pulls.review.dismissed', 'Dismissed'));
      await onDismissed();
    } catch (e) {
      toast.error(toErrorMessage(e, t('pulls.review.dismiss_failed', 'Dismiss failed')));
    } finally {
      dismissingId = null;
    }
  }
</script>

<section class="review-list">
  <h3>{t('pulls.review.history', 'Review History')}</h3>

  {#if reviewList.length === 0}
    <p class="muted">{t('pulls.review.none', 'No reviews yet.')}</p>
  {:else}
    <div class="review-items">
      {#each reviewList as review (review.id)}
        {@const info = actionInfo(review.action)}
        <div class="review-item">
          <div class="review-head">
            <span class="action-badge {info.cls}">{info.label}</span>
            <span class="reviewer">#{review.reviewer_id}</span>
            {#if review.commit_id}
              <code class="commit" title={review.commit_id}>{review.commit_id.slice(0, 7)}</code>
            {/if}
            <span class="date">{formatDate(review.created_at)}</span>
            {#if review.action === 'approve' || review.action === 'request_changes'}
              <button
                class="dismiss-btn"
                disabled={dismissingId === review.id}
                onclick={() => handleDismiss(review)}
              >{dismissingId === review.id ? '…' : t('pulls.review.dismiss', 'Dismiss')}</button>
            {/if}
          </div>
          {#if review.body}
            <p class="review-body">{review.body}</p>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</section>

<style>
  .review-list { margin-top: 24px; }
  h3 { margin: 0 0 12px; font-size: 15px; }
  .muted { color: var(--text-secondary); font-size: 13px; }

  .review-items { display: flex; flex-direction: column; gap: 8px; }

  .review-item {
    padding: 12px 14px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }

  .review-head {
    display: flex; align-items: center; gap: 10px; flex-wrap: wrap;
  }

  .action-badge {
    font-size: 12px; font-weight: 600; padding: 2px 10px;
    border-radius: 10px; border: 1px solid var(--border);
  }
  .action-badge.approved { color: #3fb950; border-color: rgba(63, 185, 80, 0.4); background: rgba(63, 185, 80, 0.08); }
  .action-badge.changes { color: #f85149; border-color: rgba(248, 81, 73, 0.4); background: rgba(248, 81, 73, 0.08); }
  .action-badge.comment { color: var(--text-secondary); }
  .action-badge.dismissed { color: var(--text-muted); border-style: dashed; }

  .reviewer { font-size: 12px; color: var(--text-secondary); }
  .commit { font-size: 11px; color: var(--text-secondary); background: var(--bg-tertiary); padding: 1px 6px; border-radius: 4px; }
  .date { font-size: 12px; color: var(--text-muted); margin-left: auto; }

  .dismiss-btn {
    font-size: 12px; padding: 2px 10px; cursor: pointer;
    background: none; border: 1px solid var(--border); border-radius: var(--radius);
    color: var(--text-secondary);
  }
  .dismiss-btn:hover:not(:disabled) { color: var(--red); border-color: var(--red-dim); }
  .dismiss-btn:disabled { opacity: 0.6; cursor: not-allowed; }

  .review-body {
    margin: 10px 0 0; font-size: 13px; color: var(--text-primary);
    white-space: pre-wrap; word-break: break-word;
  }
</style>
