<script lang="ts">
  // PR review threads (conversation tab) — root comments with replies,
  // suggestions (single apply + batch apply) and resolve/reopen actions.
  // Receives the full comment list; derives threads and owns selection state.
  import AttachmentPanel from '$lib/components/AttachmentPanel.svelte';
  import SuggestionBlock from './SuggestionBlock.svelte';
  import { reviews } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import type { PullRequest, ReviewComment } from '$lib/types/entities';

  interface Props {
    owner: string;
    repo: string;
    prNumber: number;
    pr: PullRequest;
    /** All review comments (roots + replies) for this PR. */
    comments: ReviewComment[];
    /** Called after a mutation; the parent should reload PR state. */
    onChanged: () => void | Promise<void>;
  }

  let { owner, repo, prNumber, pr, comments, onChanged }: Props = $props();

  const t = createT();

  let selectedSuggestionIds = $state<number[]>([]);
  let applyingSuggestionId = $state<number | null>(null);
  let applyingSuggestions = $state(false);
  let resolvingCommentId = $state<number | null>(null);

  let rootComments = $derived(comments.filter((comment) => !comment.reply_to_id));
  let applicableSuggestions = $derived(
    rootComments.filter(
      (comment) =>
        comment.suggestion !== null &&
        comment.suggestion !== undefined &&
        !comment.suggestion_applied_at &&
        comment.commit_id === pr.head_sha,
    ),
  );

  // Prune batch selection when the comment list is refreshed (e.g. a selected
  // suggestion was applied or the head SHA moved).
  $effect(() => {
    const list = comments;
    selectedSuggestionIds = selectedSuggestionIds.filter((id) =>
      list.some((comment) => comment.id === id && !comment.suggestion_applied_at && comment.commit_id === pr.head_sha),
    );
  });

  function repliesFor(rootId: number) {
    return comments.filter((comment) => comment.reply_to_id === rootId);
  }

  function isApplicable(comment: ReviewComment) {
    return (
      comment.suggestion !== null &&
      comment.suggestion !== undefined &&
      !comment.suggestion_applied_at &&
      comment.commit_id === pr.head_sha
    );
  }

  async function setThreadResolved(comment: ReviewComment, resolved: boolean) {
    try {
      resolvingCommentId = comment.id;
      await reviews.setThreadResolved(owner, repo, prNumber, comment.id, resolved);
      await onChanged();
    } catch (e: unknown) {
      toast.error(toErrorMessage(e));
    } finally {
      resolvingCommentId = null;
    }
  }

  async function applySuggestion(comment: ReviewComment) {
    try {
      applyingSuggestionId = comment.id;
      await reviews.applySuggestion(owner, repo, prNumber, comment.id);
      await onChanged();
    } catch (e: unknown) {
      toast.error(toErrorMessage(e));
    } finally {
      applyingSuggestionId = null;
    }
  }

  function toggleSuggestionSelection(commentId: number) {
    selectedSuggestionIds = selectedSuggestionIds.includes(commentId)
      ? selectedSuggestionIds.filter((id) => id !== commentId)
      : [...selectedSuggestionIds, commentId];
  }

  async function applySelectedSuggestions() {
    if (selectedSuggestionIds.length === 0) return;
    try {
      applyingSuggestions = true;
      await reviews.applySuggestions(owner, repo, prNumber, selectedSuggestionIds);
      selectedSuggestionIds = [];
      await onChanged();
    } catch (e: unknown) {
      toast.error(toErrorMessage(e));
    } finally {
      applyingSuggestions = false;
    }
  }
</script>

{#if applicableSuggestions.length > 1}
  <div class="suggestion-batch-bar">
    <span>{t('pulls.suggestion.batch_selected', { count: selectedSuggestionIds.length })}</span>
    <button
      class="btn-primary"
      disabled={applyingSuggestions || selectedSuggestionIds.length === 0}
      onclick={applySelectedSuggestions}
    >
      {applyingSuggestions ? t('pulls.suggestion.applying_selected') : t('pulls.suggestion.apply_selected')}
    </button>
  </div>
{/if}

{#if rootComments.length > 0}
  <section class="review-threads">
    <h3>{t('pulls.threads.title')}</h3>
    {#each rootComments as comment (comment.id)}
      <article class="thread" class:resolved={Boolean(comment.resolved_at)}>
        <header>
          <code>{comment.path}{comment.line ? `:${comment.start_line && comment.start_line !== comment.line ? `${comment.start_line}-${comment.line}` : comment.line}` : ''}</code>
          <span>{comment.resolved_at ? t('pulls.threads.resolved') : t('pulls.threads.open')}</span>
        </header>
        <div class="thread-comment">{comment.body}</div>
        <AttachmentPanel {owner} {repo} target="pulls/comments" targetId={comment.id} />
        <SuggestionBlock
          {comment}
          selectable={isApplicable(comment)}
          selected={selectedSuggestionIds.includes(comment.id)}
          onToggleSelect={() => toggleSuggestionSelection(comment.id)}
          onApply={() => applySuggestion(comment)}
          applying={applyingSuggestionId === comment.id}
        />
        {#each repliesFor(comment.id) as reply (reply.id)}
          <div class="thread-comment reply">{reply.body}</div>
          <AttachmentPanel {owner} {repo} target="pulls/comments" targetId={reply.id} />
        {/each}
        <footer>
          <button
            class="btn-secondary"
            disabled={resolvingCommentId === comment.id}
            onclick={() => setThreadResolved(comment, !comment.resolved_at)}
          >
            {comment.resolved_at ? t('pulls.threads.reopen') : t('pulls.threads.resolve')}
          </button>
        </footer>
      </article>
    {/each}
  </section>
{/if}

<style>
  .review-threads {
    margin-bottom: 16px;
    padding: 16px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  h3 { font-size: 16px; margin-bottom: 12px; }

  .thread {
    margin-top: 12px;
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  .thread.resolved { opacity: 0.72; }
  .thread header,
  .thread footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 12px;
    background: var(--bg-tertiary);
    font-size: 12px;
  }
  .thread-comment { padding: 12px; white-space: pre-wrap; }
  .thread-comment.reply {
    margin-left: 24px;
    border-top: 1px solid var(--border);
  }

  .suggestion-batch-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 16px;
    padding: 12px 16px;
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    background: rgba(88, 166, 255, 0.08);
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
