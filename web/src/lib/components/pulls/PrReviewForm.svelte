<script lang="ts">
  // PR review submit form (reviews tab) — verdict radios + comment body.
  // Self-contained: submits via the reviews API and clears itself on success.
  import { reviews } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';

  interface Props {
    owner: string;
    repo: string;
    prNumber: number;
    /** Called after a successful submit; the parent should reload PR state. */
    onSubmitted: () => void | Promise<void>;
  }

  let { owner, repo, prNumber, onSubmitted }: Props = $props();

  const t = createT();

  let reviewBody = $state('');
  let reviewVerdict = $state('comment');
  let submitting = $state(false);

  async function handleSubmitReview() {
    if (!reviewBody.trim()) return;
    try {
      submitting = true;
      await reviews.submit(owner, repo, prNumber, reviewBody, reviewVerdict);
      reviewBody = '';
      reviewVerdict = 'comment';
      await onSubmitted();
    } catch (e: unknown) {
      toast.error(toErrorMessage(e));
    } finally {
      submitting = false;
    }
  }
</script>

<div class="review-form">
  <h3>{t('pulls.review.title')}</h3>
  <div class="verdict-select">
    <label class="radio-label">
      <input type="radio" name="verdict" value="comment" bind:group={reviewVerdict} />
      {t('pulls.review.verdict_comment')}
    </label>
    <label class="radio-label">
      <input type="radio" name="verdict" value="approve" bind:group={reviewVerdict} />
      {t('pulls.review.verdict_approve')}
    </label>
    <label class="radio-label">
      <input type="radio" name="verdict" value="request_changes" bind:group={reviewVerdict} />
      {t('pulls.review.verdict_changes')}
    </label>
  </div>
  <textarea bind:value={reviewBody} rows="4" placeholder={t('pulls.review.placeholder')}></textarea>
  <button class="btn-primary" onclick={handleSubmitReview} disabled={submitting || !reviewBody.trim()}>
    {t('pulls.review.submit')}
  </button>
</div>

<style>
  .review-form {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    padding: 24px;
  }
  h3 { font-size: 16px; margin-bottom: 12px; }

  .verdict-select {
    display: flex;
    gap: 16px;
    margin-bottom: 12px;
  }

  .radio-label {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 14px;
    cursor: pointer;
  }

  textarea {
    width: 100%;
    font-family: var(--font-mono);
    font-size: 13px;
    resize: vertical;
    margin-bottom: 12px;
  }

  .btn-primary {
    padding: 6px 16px;
    background: var(--green-dim);
    color: #fff;
    border: none;
    border-radius: var(--radius);
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }
  .btn-primary:hover:not(:disabled) { background: var(--green); }
  .btn-primary:disabled { opacity: 0.5; cursor: not-allowed; }
</style>
