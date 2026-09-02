<script lang="ts">
  import { issues } from '$lib/api/client.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    owner,
    repo,
    issueNumber,
    issueState,
    onError,
    onChanged,
  }: {
    owner: string;
    repo: string;
    issueNumber: number;
    issueState: string;
    /** 表单操作失败时上抛（页面统一展示 error banner） */
    onError: (message: string) => void;
    /** 评论提交或状态切换成功后回调（页面重载） */
    onChanged: () => void | Promise<void>;
  } = $props();

  let newComment = $state('');
  let submitting = $state(false);
  let toggling = $state(false);

  async function handleComment(e: Event) {
    e.preventDefault();
    if (!newComment.trim() || submitting) return;
    try {
      submitting = true;
      await issues.addComment(owner, repo, issueNumber, newComment);
      newComment = '';
      await onChanged();
    } catch (err: unknown) {
      onError(toErrorMessage(err));
    } finally {
      submitting = false;
    }
  }

  async function toggleState() {
    if (toggling) return;
    try {
      toggling = true;
      const newState = issueState === 'open' ? 'closed' : 'open';
      await issues.update(owner, repo, issueNumber, { state: newState });
      await onChanged();
    } catch (err: unknown) {
      onError(toErrorMessage(err));
    } finally {
      toggling = false;
    }
  }
</script>

<form onsubmit={handleComment} class="comment-form">
  <textarea
    bind:value={newComment}
    rows="4"
    placeholder={t('issues.comment_placeholder')}
    disabled={submitting}
  ></textarea>
  <div class="form-actions">
    <button type="submit" class="btn-primary" disabled={!newComment.trim() || submitting}>
      {submitting ? t('common.loading') : t('issues.comment')}
    </button>
    <button type="button" class="btn-close" onclick={toggleState} disabled={toggling}>
      {issueState === 'open' ? t('issues.close_issue') : t('issues.reopen_issue')}
    </button>
  </div>
</form>

<style>
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
  .btn-primary:disabled { opacity: 0.5; cursor: not-allowed; }

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
  .btn-close:disabled { opacity: 0.5; cursor: not-allowed; }
</style>
