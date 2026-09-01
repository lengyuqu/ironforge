<script lang="ts">
  // Issue detail page — orchestration layer: loads the issue with its
  // comments and reactions, owns reaction toggling, commenting and the
  // open/close state switch. Presentation lives in lib/components/issues/.
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import AttachmentPanel from '$lib/components/AttachmentPanel.svelte';
  import { issues, type ReactionSummary } from '$lib/api/client.svelte';
  import { createT, formatDate } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import type { Issue, IssueComment } from '$lib/types/entities';
  import CommentCard from '$lib/components/issues/CommentCard.svelte';
  import AssigneesPanel from '$lib/components/issues/AssigneesPanel.svelte';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  let number = $derived(parseInt($page.params.number!));
  let issue = $state<Issue | null>(null);
  let commentList = $state<IssueComment[]>([]);
  let loading = $state(true);
  let error = $state('');
  let newComment = $state('');
  let issueReactions = $state<ReactionSummary[]>([]);
  let commentReactions = $state<Record<number, ReactionSummary[]>>({});

  $effect(() => {
    loadIssue();
  });

  async function loadIssue() {
    try {
      loading = true;
      const [issueData, commentsData, reactionsData] = await Promise.all([
        issues.get(owner, repo, number),
        issues.comments(owner, repo, number),
        issues.listReactions(owner, repo, number).catch(() => [] as ReactionSummary[]),
      ]);
      issue = issueData;
      commentList = commentsData || [];
      issueReactions = reactionsData || [];

      const commentIds = (commentsData || []).map((c) => c.id);
      const reactionEntries = await Promise.all(
        commentIds.map(async (id) => {
          const rows = await issues
            .listCommentReactions(owner, repo, id)
            .catch(() => [] as ReactionSummary[]);
          return [id, rows || []] as const;
        }),
      );
      commentReactions = Object.fromEntries(reactionEntries);
    } catch (e: unknown) {
      error = toErrorMessage(e, t('errors.load_failed', 'Load failed'));
    } finally {
      loading = false;
    }
  }

  async function toggleIssueReaction(content: string) {
    try {
      const mine = issueReactions.find((r) => r.content === content && r.reacted_by_me);
      issueReactions = mine
        ? await issues.removeReaction(owner, repo, number, content)
        : await issues.addReaction(owner, repo, number, content);
    } catch (e: unknown) {
      error = toErrorMessage(e);
    }
  }

  async function toggleCommentReaction(commentId: number, content: string) {
    try {
      const rows = commentReactions[commentId] || [];
      const mine = rows.find((r) => r.content === content && r.reacted_by_me);
      commentReactions[commentId] = mine
        ? await issues.removeCommentReaction(owner, repo, commentId, content)
        : await issues.addCommentReaction(owner, repo, commentId, content);
    } catch (e: unknown) {
      error = toErrorMessage(e);
    }
  }

  async function handleComment(e: Event) {
    e.preventDefault();
    try {
      await issues.addComment(owner, repo, number, newComment);
      newComment = '';
      await loadIssue();
    } catch (e: unknown) {
      error = toErrorMessage(e);
    }
  }

  async function toggleState() {
    if (!issue) return;
    try {
      const newState = issue.state === 'open' ? 'closed' : 'open';
      await issues.update(owner, repo, number, { state: newState });
      await loadIssue();
    } catch (e: unknown) {
      error = toErrorMessage(e);
    }
  }
</script>

<svelte:head>
  <title>{issue?.title || `${t('issues.title')} #${number}`} · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-container">
  <RepoHeader {owner} {repo} activeTab="issues" starsCount={0} />

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="text-secondary">{t('common.loading')}</p>
  {:else if issue}
    <div class="issue-detail">
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

      <AssigneesPanel {owner} {repo} issueNumber={number} />

      {#if issue.body}
        <CommentCard
          author={issue.author}
          createdAt={issue.created_at}
          body={issue.body}
          reactions={issueReactions}
          onToggleReaction={toggleIssueReaction}
        />
      {/if}

      <AttachmentPanel {owner} {repo} target="issues" targetId={number} />

      <!-- Comments -->
      {#each commentList as comment (comment.id)}
        <CommentCard
          author={comment.author}
          createdAt={comment.created_at}
          body={comment.body}
          reactions={commentReactions[comment.id] || []}
          onToggleReaction={(content) => toggleCommentReaction(comment.id, content)}
        >
          {#snippet children()}
            <AttachmentPanel {owner} {repo} target="issues/comments" targetId={comment.id} />
          {/snippet}
        </CommentCard>
      {/each}

      <!-- Add comment -->
      <form onsubmit={handleComment} class="comment-form">
        <textarea bind:value={newComment} rows="4" placeholder={t('issues.comment_placeholder')}></textarea>
        <div class="form-actions">
          <button type="submit" class="btn-primary" disabled={!newComment.trim()}>{t('issues.comment')}</button>
          <button type="button" class="btn-close" onclick={toggleState}>
            {issue.state === 'open' ? t('issues.close_issue') : t('issues.reopen_issue')}
          </button>
        </div>
      </form>
    </div>
  {/if}
</div>

<style>
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
