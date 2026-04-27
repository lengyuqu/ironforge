<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { pulls, reviews } from '$lib/api/client';
  import { createT, formatDate } from '$lib/i18n';

  const t = createT();

  let owner = $derived($page.params.owner);
  let repo = $derived($page.params.repo);
  let number = $derived(parseInt($page.params.number));
  let pr = $state<any>(null);
  let diffData = $state<string>('');
  let reviewList = $state<any[]>([]);
  let loading = $state(true);
  let error = $state('');
  let activeTab = $state('conversation');
  let mergeStrategy = $state('merge');
  let merging = $state(false);
  let reviewBody = $state('');
  let reviewVerdict = $state('comment');

  $effect(() => {
    loadPR();
  });

  async function loadPR() {
    try {
      loading = true;
      const [prData, diffResult, reviewResult] = await Promise.all([
        pulls.get(owner, repo, number),
        pulls.diff(owner, repo, number).catch(() => ({ diff: '' })),
        reviews.list(owner, repo, number).catch(() => []),
      ]);
      pr = prData;
      diffData = diffResult.diff || '';
      reviewList = reviewResult || [];
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function handleMerge() {
    try {
      merging = true;
      await pulls.merge(owner, repo, number, mergeStrategy);
      await loadPR();
    } catch (e: any) {
      error = e.message;
    } finally {
      merging = false;
    }
  }

  async function handleSubmitReview() {
    try {
      await reviews.submit(owner, repo, number, reviewBody, reviewVerdict);
      reviewBody = '';
      reviewVerdict = 'comment';
      await loadPR();
    } catch (e: any) {
      error = e.message;
    }
  }
</script>

<svelte:head>
  <title>PR #{number} · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="repo-page">
  <RepoHeader {owner} {repo} activeTab="pulls" />

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="text-secondary">{$t('common.loading')}</p>
  {:else if pr}
    <div class="pr-detail">
      <!-- Header -->
      <div class="pr-header">
        <h1>{pr.title}</h1>
        <div class="pr-meta">
          <span class="state-badge" class:open={pr.state === 'open'} class:closed={pr.state === 'closed'} class:merged={pr.state === 'merged'}>
            {$t(`pulls.state.${pr.state}`)}
          </span>
          <span class="text-secondary">
            opened {formatDate(pr.created_at)} by <strong>{pr.author || $t('common.unknown')}</strong>
          </span>
          <span class="branch-pair">
            <span class="branch-label">{pr.head_branch}</span>
            →
            <span class="branch-label">{pr.base_branch}</span>
          </span>
        </div>
      </div>

      {#if pr.body}
        <div class="pr-body">
          <div class="comment-header">
            <strong>{pr.author || $t('common.unknown')}</strong> commented
          </div>
          <div class="comment-body">{pr.body}</div>
        </div>
      {/if}

      <!-- Tabs -->
      <div class="pr-tabs">
        <button class="tab" class:active={activeTab === 'conversation'} onclick={() => activeTab = 'conversation'}>
          {$t('pulls.tabs.conversation')}
        </button>
        <button class="tab" class:active={activeTab === 'diff'} onclick={() => activeTab = 'diff'}>
          {$t('pulls.tabs.changes')}
        </button>
        <button class="tab" class:active={activeTab === 'review'} onclick={() => activeTab = 'review'}>
          {$t('pulls.tabs.reviews')} ({reviewList.length})
        </button>
      </div>

      <!-- Conversation tab -->
      {#if activeTab === 'conversation'}
        <div class="conversation">
          <!-- Merge box -->
          {#if pr.state === 'open'}
            <div class="merge-box">
              <div class="merge-row">
                <select bind:value={mergeStrategy} class="merge-select">
                  <option value="merge">{$t('pulls.merge.strategy.merge')}</option>
                  <option value="squash">{$t('pulls.merge.strategy.squash')}</option>
                  <option value="rebase">{$t('pulls.merge.strategy.rebase')}</option>
                </select>
                <button class="btn-merge" onclick={handleMerge} disabled={merging}>
                  {merging ? $t('pulls.merge.merging') : $t('pulls.merge.button')}
                </button>
              </div>
            </div>
          {/if}

          <!-- Reviews in conversation -->
          {#each reviewList as review}
            <div class="review-item">
              <div class="comment-header">
                <strong>{review.reviewer || $t('common.unknown')}</strong>
                <span class="verdict-badge" class:approved={review.verdict === 'approved'} class:changes={review.verdict === 'request_changes'} class:commented={review.verdict === 'comment'}>
                  {$t(`pulls.verdict.${review.verdict === 'request_changes' ? 'changes_requested' : review.verdict}`)}
                </span>
              </div>
              {#if review.body}
                <div class="comment-body">{review.body}</div>
              {/if}
            </div>
          {/each}
        </div>
      {/if}

      <!-- Diff tab -->
      {#if activeTab === 'diff'}
        <div class="diff-view">
          {#if diffData}
            <pre class="diff-content">{diffData}</pre>
          {:else}
            <p class="text-secondary">{$t('repo.browser.no_diff')}</p>
          {/if}
        </div>
      {/if}

      <!-- Review tab -->
      {#if activeTab === 'review'}
        <div class="review-form">
          <h3>{$t('pulls.review.title')}</h3>
          <div class="verdict-select">
            <label class="radio-label">
              <input type="radio" name="verdict" value="comment" bind:group={reviewVerdict} />
              {$t('pulls.review.verdict_comment')}
            </label>
            <label class="radio-label">
              <input type="radio" name="verdict" value="approved" bind:group={reviewVerdict} />
              {$t('pulls.review.verdict_approve')}
            </label>
            <label class="radio-label">
              <input type="radio" name="verdict" value="request_changes" bind:group={reviewVerdict} />
              {$t('pulls.review.verdict_changes')}
            </label>
          </div>
          <textarea bind:value={reviewBody} rows="4" placeholder={$t('pulls.review.placeholder')}></textarea>
          <button class="btn-primary" onclick={handleSubmitReview} disabled={!reviewBody.trim()}>
            {$t('pulls.review.submit')}
          </button>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .repo-page { max-width: 1100px; margin: 0 auto; padding: 24px; }

  .error-banner {
    background: rgba(248, 81, 73, 0.1);
    border: 1px solid var(--red-dim);
    color: var(--red);
    border-radius: var(--radius);
    padding: 10px 14px;
    font-size: 13px;
  }

  .pr-detail { max-width: 900px; }

  .pr-header { margin-bottom: 20px; }
  h1 { font-size: 24px; }
  .pr-meta { display: flex; align-items: center; gap: 8px; margin-top: 8px; font-size: 13px; }

  .state-badge {
    padding: 2px 10px;
    border-radius: 12px;
    font-size: 12px;
    font-weight: 600;
  }
  .state-badge.open { background: rgba(63, 185, 80, 0.15); color: var(--green); }
  .state-badge.closed { background: rgba(248, 81, 73, 0.15); color: var(--red); }
  .state-badge.merged { background: rgba(188, 140, 255, 0.15); color: var(--purple); }

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

  .pr-body, .review-item {
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

  .verdict-badge {
    padding: 1px 8px;
    border-radius: 10px;
    font-size: 11px;
    font-weight: 600;
  }
  .verdict-badge.approved { background: rgba(63, 185, 80, 0.15); color: var(--green); }
  .verdict-badge.changes { background: rgba(248, 81, 73, 0.15); color: var(--red); }
  .verdict-badge.commented { background: rgba(88, 166, 255, 0.15); color: var(--accent); }

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
  .tab.active { color: var(--text-primary); font-weight: 600; border-bottom-color: var(--orange); }

  .merge-box {
    background: var(--bg-secondary);
    border: 1px solid var(--green-dim);
    border-radius: var(--radius);
    padding: 16px;
    margin-bottom: 16px;
  }

  .merge-row {
    display: flex;
    gap: 8px;
  }

  .merge-select {
    padding: 6px 10px;
    font-size: 13px;
  }

  .btn-merge {
    padding: 6px 16px;
    background: var(--green-dim);
    color: #fff;
    border: none;
    border-radius: var(--radius);
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }
  .btn-merge:hover { background: var(--green); }
  .btn-merge:disabled { opacity: 0.5; }

  .diff-view {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: auto;
  }

  .diff-content {
    margin: 0;
    padding: 16px;
    font-size: 12px;
    line-height: 1.5;
    overflow-x: auto;
    border: none;
    border-radius: 0;
  }

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
  .btn-primary:hover { background: var(--green); }
  .btn-primary:disabled { opacity: 0.5; }
</style>
