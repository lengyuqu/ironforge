<script lang="ts">
  import { wiki } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT, formatDate } from '$lib/i18n';
  import type { WikiRevision } from '$lib/types/entities';

  const t = createT();

  let {
    owner,
    repo,
    title,
    onRestored,
  }: {
    owner: string;
    repo: string;
    title: string;
    onRestored: () => void | Promise<void>;
  } = $props();

  let revisions = $state<WikiRevision[]>([]);
  let historyLoading = $state(true);
  let viewingRevision = $state<WikiRevision | null>(null);
  let restoring = $state(false);

  // Load history on mount; fall back to empty list on failure.
  $effect(() => {
    const key = `${owner}/${repo}/${title}`;
    historyLoading = true;
    wiki
      .history(owner, repo, title)
      .then((revs) => {
        revisions = revs;
      })
      .catch(() => {
        revisions = [];
      })
      .finally(() => {
        historyLoading = false;
      });
    return () => {
      viewingRevision = null;
    };
  });

  async function viewRevision(rev: WikiRevision) {
    if (viewingRevision?.id === rev.id) {
      viewingRevision = null;
      return;
    }
    try {
      const full = await wiki.revision(owner, repo, title, rev.id);
      viewingRevision = full;
    } catch {
      viewingRevision = rev;
    }
  }

  async function restoreRevision(rev: WikiRevision) {
    if (!confirm(`Restore version ${rev.version}? Current content will become a revision.`)) return;
    restoring = true;
    try {
      await wiki.update(owner, repo, title, viewingRevision?.content ?? rev.content);
      toast.success(`Version ${rev.version} restored`);
      viewingRevision = null;
      await onRestored();
    } catch (e) {
      toast.error(toErrorMessage(e, t('errors.save_failed') || 'Restore failed'));
    } finally {
      restoring = false;
    }
  }
</script>

<div class="history-panel">
  <h3>Revision History</h3>
  {#if historyLoading}
    <p class="text-secondary">Loading…</p>
  {:else if revisions.length === 0}
    <p class="text-secondary">No revisions yet. Revisions are saved on every edit.</p>
  {:else}
    <div class="revision-list">
      {#each revisions as rev}
        <div class="revision-item" class:expanded={viewingRevision?.id === rev.id}>
          <button class="revision-header" onclick={() => viewRevision(rev)}>
            <span class="rev-version">v{rev.version}</span>
            <span class="rev-msg">{rev.message || 'No message'}</span>
            <span class="rev-date">{formatDate(rev.created_at)}</span>
            <span class="rev-arrow">{viewingRevision?.id === rev.id ? '▲' : '▼'}</span>
          </button>
          {#if viewingRevision?.id === rev.id}
            <div class="revision-content">
              <pre class="rev-preview">{viewingRevision.content}</pre>
              <button class="btn-primary btn-sm" disabled={restoring} onclick={() => restoreRevision(rev)}>
                Restore this version
              </button>
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .text-secondary { color: var(--text-secondary); }

  .history-panel { margin-top: 8px; }
  .history-panel h3 { font-size: 16px; font-weight: 600; margin: 0 0 16px; }
  .revision-list { display: flex; flex-direction: column; gap: 6px; }
  .revision-item { border: 1px solid var(--border); border-radius: var(--radius); overflow: hidden; }
  .revision-item.expanded { border-color: var(--accent); }
  .revision-header {
    display: flex; align-items: center; gap: 12px; padding: 10px 14px;
    background: var(--bg-primary); border: none; width: 100%; text-align: left; cursor: pointer;
  }
  .revision-header:hover { background: var(--bg-secondary); }
  .rev-version { font-size: 12px; font-weight: 700; color: var(--accent); min-width: 30px; }
  .rev-msg { flex: 1; font-size: 13px; color: var(--text-primary); }
  .rev-date { font-size: 12px; color: var(--text-muted); white-space: nowrap; }
  .rev-arrow { font-size: 10px; color: var(--text-muted); }
  .revision-content { padding: 12px 14px; border-top: 1px solid var(--border); background: var(--bg-secondary); }
  .rev-preview {
    font-size: 12px; font-family: var(--font-mono); color: var(--text-secondary);
    max-height: 200px; overflow-y: auto; white-space: pre-wrap; word-break: break-all;
    background: var(--bg-primary); border: 1px solid var(--border); border-radius: var(--radius);
    padding: 10px; margin-bottom: 10px;
  }
  .btn-primary {
    padding: 6px 16px; background: var(--accent); color: #fff;
    border: none; border-radius: var(--radius); font-size: 13px; cursor: pointer;
  }
  .btn-sm { padding: 4px 10px; font-size: 12px; }
  button:disabled { opacity: 0.6; cursor: not-allowed; }
</style>
