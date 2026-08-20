<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import AttachmentPanel from '$lib/components/AttachmentPanel.svelte';
  import { issues, type ReactionSummary } from '$lib/api/client.svelte';
  import { createT, formatDate } from '$lib/i18n';
  import { renderMarkdown as renderMarkdownSafe } from '$lib/utils/markdown';

  const t = createT();

  const REACTION_EMOJI: Record<string, string> = {
    '+1': '👍',
    '-1': '👎',
    laugh: '😄',
    confused: '😕',
    heart: '❤️',
    hooray: '🎉',
    rocket: '🚀',
    eyes: '👀',
  };

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  let number = $derived(parseInt($page.params.number!));
  let issue = $state<any>(null);
  let commentList = $state<any[]>([]);
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

      const commentIds = (commentsData || []).map((c: any) => c.id as number);
      const reactionEntries = await Promise.all(
        commentIds.map(async (id: number) => {
          const rows = await issues
            .listCommentReactions(owner, repo, id)
            .catch(() => [] as ReactionSummary[]);
          return [id, rows || []] as const;
        }),
      );
      commentReactions = Object.fromEntries(reactionEntries);
    } catch (e: any) {
      error = e.message;
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
    } catch (e: any) {
      error = e.message;
    }
  }

  async function toggleCommentReaction(commentId: number, content: string) {
    try {
      const rows = commentReactions[commentId] || [];
      const mine = rows.find((r) => r.content === content && r.reacted_by_me);
      commentReactions[commentId] = mine
        ? await issues.removeCommentReaction(owner, repo, commentId, content)
        : await issues.addCommentReaction(owner, repo, commentId, content);
    } catch (e: any) {
      error = e.message;
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

  function renderMarkdown(content: string | null | undefined): string {
    if (!content) return '';
    return renderMarkdownSafe(content);
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
            {t('issues.opened_by', { date: formatDate(issue.created_at), author: issue.author || t('common.unknown') })}
          </span>
          {#if issue.labels?.length}
            {#each issue.labels as label}
              <span class="label-badge">{label}</span>
            {/each}
          {/if}
        </div>
      </div>

      {#snippet reactionBar(rows: ReactionSummary[], onToggle: (content: string) => void)}
        <div class="reaction-bar" role="group" aria-label={t('issues.reaction.title')}>
          {#each Object.entries(REACTION_EMOJI) as [content, emoji]}
            {@const summary = rows.find((r) => r.content === content)}
            {@const count = summary?.count ?? 0}
            {@const mine = summary?.reacted_by_me ?? false}
            <button
              type="button"
              class="reaction-btn"
              class:mine
              title={t('issues.reaction.title')}
              aria-pressed={mine}
              onclick={() => onToggle(content)}
            >
              <span class="reaction-emoji">{emoji}</span>
              {#if count > 0}<span class="reaction-count">{count}</span>{/if}
            </button>
          {/each}
        </div>
      {/snippet}

      {#if issue.body}
        <div class="issue-body">
          <div class="comment-header">
            {t('issues.commented', { author: issue.author || t('common.unknown'), date: formatDate(issue.created_at) })}
          </div>
          <div class="comment-body markdown-body">{@html renderMarkdown(issue.body)}</div>
          {@render reactionBar(issueReactions, toggleIssueReaction)}
        </div>
      {/if}

      <AttachmentPanel {owner} {repo} target="issues" targetId={number} />

      <!-- Comments -->
      {#each commentList as comment}
        <div class="comment">
          <div class="comment-header">
            {t('issues.commented', { author: comment.author || t('common.unknown'), date: formatDate(comment.created_at) })}
          </div>
          <div class="comment-body markdown-body">{@html renderMarkdown(comment.body)}</div>
          {@render reactionBar(commentReactions[comment.id] || [], (content) => toggleCommentReaction(comment.id, content))}
          <AttachmentPanel {owner} {repo} target="issues/comments" targetId={comment.id} />
        </div>
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
  }

  .reaction-bar {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 0 16px 12px 16px;
  }

  .issue-body .reaction-bar,
  .comment .reaction-bar {
    border-top: 1px solid var(--border);
    padding-top: 10px;
    margin-top: 4px;
  }

  .reaction-btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px 8px;
    background: var(--bg-tertiary);
    color: var(--text-secondary);
    border: 1px solid var(--border);
    border-radius: 12px;
    font-size: 13px;
    cursor: pointer;
  }

  .reaction-btn:hover {
    background: var(--bg-hover);
    color: var(--text-primary);
  }

  .reaction-btn.mine {
    border-color: var(--green);
    color: var(--green);
    background: rgba(63, 185, 80, 0.12);
  }

  .reaction-emoji {
    font-size: 14px;
    line-height: 1;
  }

  .reaction-count {
    font-size: 12px;
    font-weight: 600;
  }

  .comment-body :global(p) {
    margin: 0 0 12px;
  }

  .comment-body :global(p:last-child) {
    margin-bottom: 0;
  }

  .comment-body :global(ul),
  .comment-body :global(ol) {
    padding-left: 24px;
    margin: 8px 0 12px;
  }

  .comment-body :global(pre) {
    overflow-x: auto;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 12px;
  }

  .comment-body :global(code) {
    font-family: var(--font-mono);
    font-size: 12px;
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
