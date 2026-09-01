<script lang="ts">
  // Merge box — the merge / auto-merge / merge-queue control panel for an open
  // PR, plus the repository merge-queue summary. Self-contained: performs the
  // merge actions itself and notifies the parent via `onChanged` to reload.
  import { pulls, type MergeQueueEntry } from '$lib/api/pulls';
  import { createT } from '$lib/i18n';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import type { PullRequest } from '$lib/types/entities';

  interface Props {
    owner: string;
    repo: string;
    prNumber: number;
    pr: PullRequest;
    mergeQueue: MergeQueueEntry[];
    /** Called after a successful action; the parent should reload PR state. */
    onChanged: () => void | Promise<void>;
  }

  let { owner, repo, prNumber, pr, mergeQueue, onChanged }: Props = $props();

  const t = createT();

  let mergeStrategy = $state('merge');
  let merging = $state(false);
  let managingAutoMerge = $state(false);
  let managingMergeQueue = $state(false);
  let autoMergeReason = $state('');

  let queuedEntry = $derived(mergeQueue.find((entry) => entry.pr_number === prNumber));

  async function handleMerge() {
    try {
      merging = true;
      await pulls.merge(owner, repo, prNumber, mergeStrategy);
      await onChanged();
    } catch (e: unknown) {
      toast.error(toErrorMessage(e));
    } finally {
      merging = false;
    }
  }

  async function enableAutoMerge() {
    try {
      managingAutoMerge = true;
      const outcome = await pulls.enableAutoMerge(owner, repo, prNumber, mergeStrategy);
      autoMergeReason = outcome.reason || '';
      await onChanged();
    } catch (e: unknown) {
      toast.error(toErrorMessage(e));
    } finally {
      managingAutoMerge = false;
    }
  }

  async function disableAutoMerge() {
    try {
      managingAutoMerge = true;
      await pulls.disableAutoMerge(owner, repo, prNumber);
      autoMergeReason = '';
      await onChanged();
    } catch (e: unknown) {
      toast.error(toErrorMessage(e));
    } finally {
      managingAutoMerge = false;
    }
  }

  async function enqueueMerge() {
    try {
      managingMergeQueue = true;
      await pulls.enqueueMerge(owner, repo, prNumber, mergeStrategy);
      await onChanged();
    } catch (e: unknown) {
      toast.error(toErrorMessage(e));
    } finally {
      managingMergeQueue = false;
    }
  }

  async function cancelQueuedMerge() {
    try {
      managingMergeQueue = true;
      await pulls.cancelQueuedMerge(owner, repo, prNumber);
      await onChanged();
    } catch (e: unknown) {
      toast.error(toErrorMessage(e));
    } finally {
      managingMergeQueue = false;
    }
  }
</script>

{#if pr.is_draft}
  <div class="merge-box">
    <div class="draft-notice">{t('pulls.merge.draft_blocked')}</div>
  </div>
{:else if queuedEntry}
  <div class="merge-box">
    <div class="auto-merge-pending">
      <div>
        <strong>{t('pulls.merge.queue_position', { position: queuedEntry.position })}</strong>
        <span>{t('pulls.merge.queue_waiting', { strategy: queuedEntry.strategy })}</span>
      </div>
      <button class="btn-secondary" onclick={cancelQueuedMerge} disabled={managingMergeQueue || queuedEntry.status === 'running'}>
        {t('pulls.merge.leave_queue')}
      </button>
    </div>
  </div>
{:else if pr.auto_merge_enabled}
  <div class="merge-box">
    <div class="auto-merge-pending">
      <div>
        <strong>{t('pulls.merge.auto_enabled')}</strong>
        <span>{t('pulls.merge.auto_waiting', { strategy: pr.auto_merge_strategy ?? '' })}</span>
        {#if autoMergeReason}<small>{autoMergeReason}</small>{/if}
      </div>
      <button class="btn-secondary" onclick={disableAutoMerge} disabled={managingAutoMerge}>
        {t('pulls.merge.disable_auto')}
      </button>
    </div>
  </div>
{:else}
  <div class="merge-box">
    <div class="merge-row">
      <select bind:value={mergeStrategy} class="merge-select">
        <option value="merge">{t('pulls.merge.strategy.merge')}</option>
        <option value="squash">{t('pulls.merge.strategy.squash')}</option>
        <option value="rebase">{t('pulls.merge.strategy.rebase')}</option>
      </select>
      <button class="btn-merge" onclick={handleMerge} disabled={merging}>
        {merging ? t('pulls.merge.merging') : t('pulls.merge.button')}
      </button>
      <button class="btn-secondary" onclick={enableAutoMerge} disabled={managingAutoMerge}>
        {managingAutoMerge ? t('pulls.merge.enabling_auto') : t('pulls.merge.enable_auto')}
      </button>
      <button class="btn-secondary" onclick={enqueueMerge} disabled={managingMergeQueue}>
        {managingMergeQueue ? t('pulls.merge.joining_queue') : t('pulls.merge.join_queue')}
      </button>
    </div>
  </div>
{/if}
{#if mergeQueue.length > 0}
  <div class="queue-summary">
    <strong>{t('pulls.merge.queue_title')}</strong>
    {#each mergeQueue.slice(0, 5) as entry (entry.id)}
      <span>#{entry.position} · PR #{entry.pr_number} · {entry.title}</span>
    {/each}
  </div>
{/if}

<style>
  .merge-box {
    background: var(--bg-secondary);
    border: 1px solid var(--green-dim);
    border-radius: var(--radius);
    padding: 16px;
    margin-bottom: 16px;
  }

  .draft-notice { color: var(--text-secondary); }

  .auto-merge-pending {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }
  .auto-merge-pending > div { display: flex; flex-direction: column; gap: 4px; }
  .auto-merge-pending small { color: var(--text-secondary); }

  .merge-row { display: flex; gap: 8px; }

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
  .btn-merge:hover:not(:disabled) { background: var(--green); }
  .btn-merge:disabled { opacity: 0.5; cursor: not-allowed; }

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

  .queue-summary {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: -8px 0 16px;
    padding: 10px 16px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text-secondary);
    font-size: 12px;
  }
</style>
