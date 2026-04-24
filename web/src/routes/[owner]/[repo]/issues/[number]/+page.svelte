<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { issues } from '$lib/api/client';

  let owner = $derived($page.params.owner);
  let repo = $derived($page.params.repo);
  let number = $derived(parseInt($page.params.number));
  let issue = $state<any>(null);
  let commentList = $state<any[]>([]);
  let loading = $state(true);
  let error = $state('');
  let newComment = $state('');

  $effect(() => {
    loadIssue();
  });

  async function loadIssue() {
    try {
      loading = true;
      const [issueData, commentsData] = await Promise.all([
        issues.get(owner, repo, number),
        issues.comments(owner, repo, number),
      ]);
      issue = issueData;
      commentList = commentsData || [];
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function handleComment(e: Event) {
    e.preventDefault();
    try {
      await issues.addComment(owner, repo, number, newComment);
      newComment = '';
      await loadIssue();
    } catch (e: any) {
      error = e.message;
    }
  }

  async function toggleState() {
    try {
      const newState = issue.state === 'open' ? 'closed' : 'open';
      await issues.update(owner, repo, number, { state: newState });
      await loadIssue();
    } catch (e: any) {
      error = e.message;
    }
  }

  function formatDate(dateStr: string) {
    return new Date(dateStr).toLocaleDateString('en-US', { year: 'numeric', month: 'short', day: 'numeric' });
  }
</script>

<svelte:head>
  <title>Issue #{number} · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="repo-page">
  <RepoHeader {owner} {repo} activeTab="issues" />

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="text-secondary">Loading issue...</p>
  {:else if issue}
    <div class="issue-detail">
      <div class="issue-header">
        <div class="issue-title-row">
          <h1>{issue.title}</h1>
          <span class="issue-number">#{issue.number}</span>
        </div>
        <div class="issue-meta">
          <span class="state-badge" class:open={issue.state === 'open'} class:closed={issue.state === 'closed'}>
            {issue.state === 'open' ? '● Open' : '✓ Closed'}
          </span>
          <span class="text-secondary">
            opened {formatDate(issue.created_at)} by <strong>{issue.author || 'unknown'}</strong>
          </span>
          {#if issue.labels?.length}
            {#each issue.labels as label}
              <span class="label-badge">{label}</span>
            {/each}
          {/if}
        </div>
      </div>

      {#if issue.body}
        <div class="issue-body">
          <div class="comment-header">
            <strong>{issue.author || 'unknown'}</strong> commented {formatDate(issue.created_at)}
          </div>
          <div class="comment-body">{issue.body}</div>
        </div>
      {/if}

      <!-- Comments -->
      {#each commentList as comment}
        <div class="comment">
          <div class="comment-header">
            <strong>{comment.author || 'unknown'}</strong> commented {formatDate(comment.created_at)}
          </div>
          <div class="comment-body">{comment.body}</div>
        </div>
      {/each}

      <!-- Add comment -->
      <form onsubmit={handleComment} class="comment-form">
        <textarea bind:value={newComment} rows="4" placeholder="Add a comment..."></textarea>
        <div class="form-actions">
          <button type="submit" class="btn-primary" disabled={!newComment.trim()}>Comment</button>
          <button type="button" class="btn-close" onclick={toggleState}>
            {issue.state === 'open' ? 'Close Issue' : 'Reopen Issue'}
          </button>
        </div>
      </form>
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

  .issue-detail { max-width: 800px; }

  .issue-header { margin-bottom: 24px; }

  .issue-title-row {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }
  h1 { font-size: 24px; }
  .issue-number { color: var(--text-muted); font-size: 18px; }

  .issue-meta {
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

  .label-badge {
    display: inline-block;
    padding: 0 6px;
    border: 1px solid var(--purple);
    color: var(--purple);
    border-radius: 10px;
    font-size: 11px;
  }

  .issue-body, .comment {
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
  }

  .comment-body {
    padding: 16px;
    font-size: 14px;
    line-height: 1.6;
    white-space: pre-wrap;
  }

  .comment-form {
    margin-top: 16px;
  }

  textarea {
    width: 100%;
    font-family: var(--font-mono);
    font-size: 13px;
    resize: vertical;
    margin-bottom: 8px;
  }

  .form-actions {
    display: flex;
    gap: 8px;
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

  .btn-close {
    padding: 6px 16px;
    background: none;
    color: var(--text-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    font-size: 14px;
    cursor: pointer;
  }
  .btn-close:hover { background: var(--bg-hover); }
</style>
