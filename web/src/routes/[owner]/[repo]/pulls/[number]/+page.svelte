<script lang="ts">
  // PR detail page — orchestrator. Loads PR state and delegates the heavy
  // UI to focused components under $lib/components/pulls/:
  //   PrReviewersBox / PrMergeBox / PrTimeline / PrThreads / PrDiffView / PrReviewForm
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import AttachmentPanel from '$lib/components/AttachmentPanel.svelte';
  import PrReviewersBox from '$lib/components/pulls/PrReviewersBox.svelte';
  import PrMergeBox from '$lib/components/pulls/PrMergeBox.svelte';
  import PrTimeline from '$lib/components/pulls/PrTimeline.svelte';
  import PrThreads from '$lib/components/pulls/PrThreads.svelte';
  import PrDiffView from '$lib/components/pulls/PrDiffView.svelte';
  import PrReviewForm from '$lib/components/pulls/PrReviewForm.svelte';
  import { pulls, reviews } from '$lib/api/client.svelte';
  import PrReviewList from '$lib/components/pulls/PrReviewList.svelte';
  import type { MergeQueueEntry, PrDiff } from '$lib/api/pulls';
  import { createT, formatDate } from '$lib/i18n';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import type { PrTimelineEvent, PrReview, PullRequest, ReviewComment } from '$lib/types/entities';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  let number = $derived(parseInt($page.params.number!));
  let pr = $state<PullRequest | null>(null);
  let diffData = $state<PrDiff | null>(null);
  let reviewList = $state<PrReview[]>([]);
  let reviewComments = $state<ReviewComment[]>([]);
  let timeline = $state<PrTimelineEvent[]>([]);
  let mergeQueue = $state<MergeQueueEntry[]>([]);
  let loading = $state(true);
  let error = $state('');
  let activeTab = $state('conversation');
  let updatingDraft = $state(false);

  $effect(() => {
    loadPR();
  });

  async function loadPR() {
    try {
      loading = true;
      const [prData, diffResult, reviewResult, commentsResult, timelineResult, queueResult] = await Promise.all([
        pulls.get(owner, repo, number),
        pulls.diff(owner, repo, number).catch(() => null),
        reviews.list(owner, repo, number).catch(() => []),
        reviews.comments(owner, repo, number).catch(() => []),
        reviews.timeline(owner, repo, number).catch(() => []),
        pulls.mergeQueue(owner, repo).catch(() => []),
      ]);
      pr = prData;
      diffData = diffResult;
      reviewList = reviewResult || [];
      reviewComments = commentsResult || [];
      timeline = timelineResult || [];
      mergeQueue = queueResult || [];
    } catch (e: unknown) {
      error = toErrorMessage(e);
    } finally {
      loading = false;
    }
  }

  async function toggleDraft() {
    if (!pr) return;
    try {
      updatingDraft = true;
      pr = await pulls.update(owner, repo, number, { draft: !pr.is_draft });
    } catch (e: unknown) {
      toast.error(toErrorMessage(e));
    } finally {
      updatingDraft = false;
    }
  }
</script>

<svelte:head>
  <title>PR #{number} · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-container">
  <RepoHeader {owner} {repo} activeTab="pulls" starsCount={0} />

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="text-secondary">{t('common.loading')}</p>
  {:else if pr}
    <div class="pr-detail">
      <!-- Header -->
      <div class="pr-header">
        <h1>{pr.title}</h1>
        <div class="pr-meta">
          <span class="state-badge" class:open={pr.state === 'open'} class:closed={pr.state === 'closed'} class:merged={pr.state === 'merged'}>
            {t(`pulls.state.${pr.state}`)}
          </span>
          {#if pr.is_draft}<span class="draft-badge">{t('pulls.draft')}</span>{/if}
          <span class="text-secondary">
            opened {formatDate(pr.created_at!)} by <strong>{pr.author || t('common.unknown')}</strong>
          </span>
          <span class="branch-pair">
            <span class="branch-label">{pr.head_branch}</span>
            →
            <span class="branch-label">{pr.base_branch}</span>
          </span>
          {#if pr.state === 'open'}
            <button class="btn-link" onclick={toggleDraft} disabled={updatingDraft}>
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

      <AttachmentPanel {owner} {repo} target="pulls" targetId={number} />

      <!-- Tabs -->
      <div class="pr-tabs">
        <button class="tab" class:active={activeTab === 'conversation'} onclick={() => activeTab = 'conversation'}>
          {t('pulls.tabs.conversation')}
        </button>
        <button class="tab" class:active={activeTab === 'diff'} onclick={() => activeTab = 'diff'}>
          {t('pulls.tabs.changes')}
        </button>
        <button class="tab" class:active={activeTab === 'review'} onclick={() => activeTab = 'review'}>
          {t('pulls.tabs.reviews')} ({reviewList.length})
        </button>
      </div>

      <!-- Conversation tab -->
      {#if activeTab === 'conversation'}
        <div class="conversation">
          <PrReviewersBox {owner} {repo} prNumber={number} />

          {#if pr.state === 'open'}
            <PrMergeBox {owner} {repo} prNumber={number} {pr} {mergeQueue} onChanged={loadPR} />
          {/if}

          {#if timeline.length > 0}
            <PrTimeline events={timeline} />
          {/if}

          <PrThreads {owner} {repo} prNumber={number} {pr} comments={reviewComments} onChanged={loadPR} />
        </div>
      {/if}

      <!-- Diff tab -->
      {#if activeTab === 'diff'}
        <PrDiffView {owner} {repo} prNumber={number} {pr} {diffData} comments={reviewComments} onChanged={loadPR} />
      {/if}

      <!-- Review tab -->
      {#if activeTab === 'review'}
        <PrReviewForm {owner} {repo} prNumber={number} onSubmitted={loadPR} />
        <PrReviewList {owner} {repo} prNumber={number} reviews={reviewList} onDismissed={loadPR} />
      {/if}
    </div>
  {/if}
</div>

<style>
  .pr-detail { max-width: 1200px; }

  .pr-header { margin-bottom: 20px; }
  h1 { font-size: 24px; }
  .pr-meta {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
    font-size: 13px;
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

  .pr-tabs {
    display: flex;
    gap: 0;
    border-bottom: 1px solid var(--border);
    margin-bottom: 16px;
  }

  .tab {
    padding: 8px 16px;
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    color: var(--text-secondary);
    font-size: 14px;
    cursor: pointer;
  }
  .tab.active {
    color: var(--text-primary);
    font-weight: 600;
    border-bottom-color: var(--orange);
  }

</style>
