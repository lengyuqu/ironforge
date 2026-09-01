<script lang="ts">
  // Reviewers box — manages the requested-reviewer list for a PR.
  // Self-contained: loads/manages its own reviewer state via the reviews API.
  import { reviews } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import type { RequestedReviewer } from '$lib/types/entities';

  interface Props {
    owner: string;
    repo: string;
    prNumber: number;
  }

  let { owner, repo, prNumber }: Props = $props();

  const t = createT();

  let reviewers = $state<RequestedReviewer[]>([]);
  let reviewerUsername = $state('');
  let managing = $state(false);
  let loading = $state(true);

  $effect(() => {
    loadReviewers();
  });

  async function loadReviewers() {
    try {
      loading = true;
      reviewers = await reviews.requestedReviewers(owner, repo, prNumber);
    } catch (e: unknown) {
      toast.error(toErrorMessage(e));
    } finally {
      loading = false;
    }
  }

  async function requestReviewer() {
    if (!reviewerUsername.trim()) return;
    try {
      managing = true;
      await reviews.requestReviewer(owner, repo, prNumber, reviewerUsername.trim());
      reviewerUsername = '';
      reviewers = await reviews.requestedReviewers(owner, repo, prNumber);
    } catch (e: unknown) {
      toast.error(toErrorMessage(e));
    } finally {
      managing = false;
    }
  }

  async function removeReviewer(username: string) {
    try {
      managing = true;
      await reviews.removeRequestedReviewer(owner, repo, prNumber, username);
      reviewers = reviewers.filter((reviewer) => reviewer.username !== username);
    } catch (e: unknown) {
      toast.error(toErrorMessage(e));
    } finally {
      managing = false;
    }
  }
</script>

<section class="reviewers-box">
  <h3>{t('pulls.reviewers.title')}</h3>
  {#if loading}
    <p class="text-secondary">{t('common.loading')}</p>
  {:else if reviewers.length === 0}
    <p class="text-secondary">{t('pulls.reviewers.empty')}</p>
  {:else}
    <div class="reviewer-list">
      {#each reviewers as reviewer (reviewer.id)}
        <span class="reviewer-chip">
          @{reviewer.username}
          <button
            aria-label={t('pulls.reviewers.remove', { username: reviewer.username })}
            disabled={managing}
            onclick={() => removeReviewer(reviewer.username)}
          >×</button>
        </span>
      {/each}
    </div>
  {/if}
  <div class="reviewer-form">
    <input bind:value={reviewerUsername} placeholder={t('pulls.reviewers.placeholder')} />
    <button class="btn-secondary" onclick={requestReviewer} disabled={managing || !reviewerUsername.trim()}>
      {t('pulls.reviewers.request')}
    </button>
  </div>
</section>

<style>
  .reviewers-box {
    margin-bottom: 16px;
    padding: 16px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  h3 { font-size: 16px; margin-bottom: 12px; }

  .reviewer-list {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-bottom: 12px;
  }
  .reviewer-chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 3px 8px;
    border-radius: 12px;
    background: var(--bg-tertiary);
    font-size: 13px;
  }
  .reviewer-chip button {
    padding: 0;
    border: none;
    background: none;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .reviewer-form { display: flex; gap: 8px; margin-top: 12px; }
  .reviewer-form input { flex: 1; min-width: 0; }

  .btn-secondary {
    padding: 6px 16px;
    background: var(--bg-tertiary);
    color: var(--text-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 14px;
    cursor: pointer;
  }
  .btn-secondary:hover:not(:disabled) { border-color: var(--accent); }
  .btn-secondary:disabled { opacity: 0.5; cursor: not-allowed; }
</style>
