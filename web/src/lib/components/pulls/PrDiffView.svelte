<script lang="ts">
  // PR diff view (changes tab) — file-by-file diff with jump list, collapsible
  // file sections, per-line inline comments, reply threads and suggestions.
  import Icon from '$lib/components/Icon.svelte';
  import SuggestionBlock from './SuggestionBlock.svelte';
  import { reviews, type DiffLine, type PrDiff } from '$lib/api/pulls';
  import { createT } from '$lib/i18n';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import type { PullRequest, ReviewComment } from '$lib/types/entities';

  interface Props {
    owner: string;
    repo: string;
    prNumber: number;
    pr: PullRequest;
    diffData: PrDiff | null;
    /** All review comments (roots + replies) for this PR. */
    comments: ReviewComment[];
    /** Called after a mutation; the parent should reload PR state. */
    onChanged: () => void | Promise<void>;
  }

  let { owner, repo, prNumber, pr, diffData, comments, onChanged }: Props = $props();

  const t = createT();

  let commentTarget = $state<{ key: string; path: string; line: number; side: 'LEFT' | 'RIGHT' } | null>(null);
  let inlineCommentBody = $state('');
  let submittingInlineComment = $state(false);
  let suggestingChange = $state(false);
  let suggestedContent = $state('');
  let suggestionLineCount = $state(1);
  let applyingSuggestionId = $state<number | null>(null);
  let resolvingCommentId = $state<number | null>(null);
  let collapsedFiles = $state<Record<string, boolean>>({});

  let rootComments = $derived(comments.filter((comment) => !comment.reply_to_id));
  let allCollapsed = $derived(
    Boolean(diffData?.files_changed.length && diffData.files_changed.every((f) => collapsedFiles[f.path])),
  );

  function toggleFile(filePath: string) {
    collapsedFiles = { ...collapsedFiles, [filePath]: !collapsedFiles[filePath] };
  }

  function toggleAllFiles() {
    if (!diffData) return;
    const next = !allCollapsed;
    const updated: Record<string, boolean> = {};
    for (const file of diffData.files_changed) {
      updated[file.path] = next;
    }
    collapsedFiles = updated;
  }

  function jumpToFile(filePath: string, index: number) {
    if (collapsedFiles[filePath]) {
      collapsedFiles = { ...collapsedFiles, [filePath]: false };
    }
    document.getElementById(`diff-file-${index}`)?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }

  function commentLocation(path: string, line: DiffLine, index: number) {
    if (line.kind === 'deletion' && line.old_line != null) {
      return { key: `${path}:${index}`, path, line: line.old_line, side: 'LEFT' as const };
    }
    if ((line.kind === 'addition' || line.kind === 'context') && line.new_line != null) {
      return { key: `${path}:${index}`, path, line: line.new_line, side: 'RIGHT' as const };
    }
    return null;
  }

  function commentsForLine(path: string, line: number, side: 'LEFT' | 'RIGHT') {
    return rootComments.filter(
      (comment) => comment.path === path && comment.line === line && (!comment.side || comment.side === side),
    );
  }

  function repliesFor(rootId: number) {
    return comments.filter((comment) => comment.reply_to_id === rootId);
  }

  function startInlineComment(target: { key: string; path: string; line: number; side: 'LEFT' | 'RIGHT' }, content: string) {
    commentTarget = target;
    inlineCommentBody = '';
    suggestingChange = false;
    suggestedContent = content;
    suggestionLineCount = 1;
  }

  async function submitInlineComment() {
    if (!commentTarget || !inlineCommentBody.trim()) return;
    try {
      submittingInlineComment = true;
      const rangeLength = Math.min(100, Math.max(1, Number(suggestionLineCount) || 1));
      await reviews.addComment(owner, repo, prNumber, {
        path: commentTarget.path,
        line: suggestingChange ? commentTarget.line + rangeLength - 1 : commentTarget.line,
        start_line: suggestingChange ? commentTarget.line : undefined,
        side: commentTarget.side,
        start_side: suggestingChange ? commentTarget.side : undefined,
        body: inlineCommentBody.trim(),
        suggestion: suggestingChange ? suggestedContent : undefined,
      });
      await onChanged();
      commentTarget = null;
      inlineCommentBody = '';
      suggestingChange = false;
      suggestedContent = '';
      suggestionLineCount = 1;
    } catch (e: unknown) {
      toast.error(toErrorMessage(e));
    } finally {
      submittingInlineComment = false;
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
</script>

<div class="diff-view">
  {#if diffData && diffData.files_changed.length > 0}
    <div class="diff-summary">
      <div class="diff-summary-meta">
        <strong>{t('pulls.files_changed_count', { count: diffData.stats.files_changed })}</strong>
        <span class="addition-text">+{diffData.stats.total_additions}</span>
        <span class="deletion-text">−{diffData.stats.total_deletions}</span>
      </div>
      <button type="button" class="btn-outline btn-xs" onclick={toggleAllFiles}>
        {allCollapsed ? t('pulls.diff.expand_all') : t('pulls.diff.collapse_all')}
      </button>
    </div>

    {#if diffData.files_changed.length > 1}
      <nav class="diff-jump-list surface" aria-label={t('pulls.diff.jump_to_file')}>
        {#each diffData.files_changed as file, fileIdx (file.path)}
          <button type="button" class="diff-jump-item" onclick={() => jumpToFile(file.path, fileIdx)}>
            <code class="truncate">{file.path}</code>
            <span class="jump-stats">
              <span class="addition-text">+{file.additions}</span>
              <span class="deletion-text">−{file.deletions}</span>
            </span>
          </button>
        {/each}
      </nav>
    {/if}

    {#each diffData.files_changed as file, fileIdx (file.path)}
      {@const collapsed = Boolean(collapsedFiles[file.path])}
      <section class="diff-file" id={`diff-file-${fileIdx}`}>
        <header class="diff-file-header">
          <button
            type="button"
            class="file-toggle"
            aria-expanded={!collapsed}
            aria-label={`${t('pulls.diff.toggle_file')}: ${file.path}`}
            onclick={() => toggleFile(file.path)}
          >
            <Icon name={collapsed ? 'chevron-right' : 'chevron-down'} size={14} />
            <code>{file.path}</code>
          </button>
          <span class="file-stats">
            <span class="addition-text">+{file.additions}</span>
            <span class="deletion-text">−{file.deletions}</span>
          </span>
        </header>
        {#if !collapsed}
          <div class="diff-lines">
            {#each file.lines as line, index (`${file.path}:${index}`)}
              {@const target = commentLocation(file.path, line, index)}
              {@const lineComments = target ? commentsForLine(file.path, target.line, target.side) : []}
              <div class="diff-line" class:addition={line.kind === 'addition'} class:deletion={line.kind === 'deletion'} class:meta={line.kind === 'meta'}>
                <span class="comment-gutter">
                  {#if target}
                    <button title={t('pulls.diff.add_comment')} aria-label={t('pulls.diff.add_comment')} onclick={() => startInlineComment(target, line.content)}>+</button>
                  {/if}
                </span>
                <span class="line-number">{line.old_line ?? ''}</span>
                <span class="line-number">{line.new_line ?? ''}</span>
                <code>{line.content || ' '}</code>
              </div>
              {#each lineComments as comment (comment.id)}
                <div class="inline-thread" class:resolved={Boolean(comment.resolved_at)}>
                  <div>{comment.body}</div>
                  <SuggestionBlock
                    {comment}
                    onApply={() => applySuggestion(comment)}
                    applying={applyingSuggestionId === comment.id}
                  />
                  {#each repliesFor(comment.id) as reply (reply.id)}
                    <div class="inline-reply">{reply.body}</div>
                  {/each}
                  <button class="btn-link" disabled={resolvingCommentId === comment.id} onclick={() => setThreadResolved(comment, !comment.resolved_at)}>
                    {comment.resolved_at ? t('pulls.threads.reopen') : t('pulls.threads.resolve')}
                  </button>
                </div>
              {/each}
              {#if target && commentTarget?.key === target.key}
                <div class="inline-comment-form">
                  <textarea bind:value={inlineCommentBody} rows="3" placeholder={t('pulls.diff.comment_placeholder')}></textarea>
                  {#if target.side === 'RIGHT'}
                    <label class="suggestion-toggle"><input type="checkbox" bind:checked={suggestingChange} /> {t('pulls.suggestion.propose')}</label>
                    {#if suggestingChange}
                      <label class="range-control">
                        {t('pulls.suggestion.line_count')}
                        <input type="number" min="1" max="100" bind:value={suggestionLineCount} />
                      </label>
                      <textarea bind:value={suggestedContent} rows="4" placeholder={t('pulls.suggestion.placeholder')}></textarea>
                    {/if}
                  {/if}
                  <div>
                    <button class="btn-primary" disabled={submittingInlineComment || !inlineCommentBody.trim()} onclick={submitInlineComment}>{t('pulls.diff.submit_comment')}</button>
                    <button class="btn-secondary" disabled={submittingInlineComment} onclick={() => (commentTarget = null)}>{t('common.cancel')}</button>
                  </div>
                </div>
              {/if}
            {/each}
          </div>
        {/if}
      </section>
    {/each}
  {:else}
    <p class="text-secondary">{t('repo.browser.no_diff')}</p>
  {/if}
</div>

<style>
  .diff-view {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .diff-summary {
    display: flex;
    justify-content: space-between;
    gap: 10px;
    align-items: center;
    flex-wrap: wrap;
  }

  .diff-summary-meta {
    display: flex;
    gap: 10px;
    align-items: center;
  }

  .diff-jump-list {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding: 10px 12px;
  }

  .diff-jump-item {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    max-width: 100%;
    padding: 4px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-primary);
    color: var(--text-primary);
    font-size: 12px;
    cursor: pointer;
  }

  .diff-jump-item:hover {
    border-color: var(--accent);
    background: var(--bg-hover);
  }

  .jump-stats {
    display: inline-flex;
    gap: 4px;
    flex-shrink: 0;
  }

  .addition-text { color: var(--green); }
  .deletion-text { color: var(--red); }

  .diff-file {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .diff-file-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
    background: var(--bg-tertiary);
    border-bottom: 1px solid var(--border);
    font-size: 13px;
  }

  .file-toggle {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    padding: 0;
    border: none;
    background: none;
    color: var(--text-primary);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .file-toggle code {
    overflow-wrap: anywhere;
  }

  .file-stats {
    display: inline-flex;
    gap: 6px;
    flex-shrink: 0;
  }

  .diff-lines { overflow-x: auto; }

  .diff-line {
    display: grid;
    grid-template-columns: 28px 48px 48px minmax(max-content, 1fr);
    min-height: 22px;
    font-size: 12px;
    line-height: 22px;
    background: var(--bg-primary);
  }
  .diff-line.addition { background: color-mix(in srgb, var(--green) 12%, transparent); }
  .diff-line.deletion { background: color-mix(in srgb, var(--red) 12%, transparent); }
  .diff-line.meta { background: color-mix(in srgb, var(--accent) 9%, transparent); color: var(--text-secondary); }
  .diff-line > code {
    padding: 0 10px;
    white-space: pre;
    border-left: 1px solid var(--border);
  }

  .line-number {
    padding: 0 6px;
    color: var(--text-secondary);
    text-align: right;
    user-select: none;
    border-left: 1px solid var(--border);
  }

  .comment-gutter {
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .comment-gutter button {
    width: 20px;
    height: 20px;
    padding: 0;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: transparent;
    cursor: pointer;
  }
  .diff-line:hover .comment-gutter button,
  .comment-gutter button:focus-visible {
    color: var(--text-on-accent);
    background: var(--accent);
  }

  @media (hover: none) {
    .comment-gutter button {
      color: var(--text-secondary);
      background: var(--bg-tertiary);
    }
  }

  .inline-comment-form,
  .inline-thread {
    margin: 8px 12px 8px 124px;
    padding: 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-secondary);
  }
  .inline-comment-form textarea { margin-bottom: 8px; }
  .inline-comment-form > div { display: flex; gap: 8px; }
  .inline-thread.resolved { opacity: 0.72; }

  .inline-reply {
    margin: 8px 0 8px 16px;
    padding-top: 8px;
    border-top: 1px solid var(--border);
  }

  .suggestion-toggle {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 8px;
    font-size: 13px;
  }

  .range-control {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
    font-size: 13px;
  }
  .range-control input { width: 72px; }

  textarea {
    width: 100%;
    resize: vertical;
    margin-bottom: 12px;
  }

  @media (max-width: 720px) {
    .inline-comment-form,
    .inline-thread {
      margin: 8px;
    }
  }
</style>
