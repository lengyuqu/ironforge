<script lang="ts">
  import PipelineBadge from '$lib/components/PipelineBadge.svelte';
  import PipelineFlow from '$lib/components/pipelines/PipelineFlow.svelte';
  import ArtifactsPanel from '$lib/components/pipelines/ArtifactsPanel.svelte';
  import type { PipelineDetail } from '$lib/types/entities';
  import { createT } from '$lib/i18n';
  import { formatDuration } from '$lib/utils/pipelineStatus';

  const t = createT();

  let {
    owner,
    repo,
    pipeline,
    approvedJobs,
    onRetry,
    onCancel,
    onOpenJobLog,
    onPlayJob,
    onApproveJob,
    onRerunJob,
  }: {
    owner: string;
    repo: string;
    pipeline: PipelineDetail;
    approvedJobs: number[];
    onRetry: (id: number) => void;
    onCancel: (id: number) => void;
    onOpenJobLog: (jobId: number) => void;
    onPlayJob: (jobId: number) => void;
    onApproveJob: (jobId: number) => void;
    onRerunJob: (jobId: number) => void;
  } = $props();

  let retryable = $derived(
    pipeline.status === 'failed' || pipeline.status === 'failure' || pipeline.status === 'error'
  );
  let cancellable = $derived(
    pipeline.status === 'running' ||
      pipeline.status === 'pending' ||
      pipeline.status === 'manual' ||
      pipeline.status === 'waiting_approval'
  );
</script>

<div class="detail-header">
  <h2>{t('pipeline.detail_title', { id: String(pipeline.id) })}</h2>
  <PipelineBadge status={pipeline.status} />
  <div class="detail-actions">
    {#if retryable}
      <button class="btn-outline" onclick={() => onRetry(pipeline.id)}>{t('pipeline.retry')}</button>
    {/if}
    {#if cancellable}
      <button class="btn-outline btn-danger" onclick={() => onCancel(pipeline.id)}
        >{t('pipeline.cancel')}</button
      >
    {/if}
  </div>
</div>

<div class="detail-info">
  <div>
    <span class="text-secondary">{t('pipeline.commit')}:</span>
    <code>{pipeline.commit_sha?.slice(0, 7)}</code>
  </div>
  <div><span class="text-secondary">{t('pipeline.branch')}:</span> {pipeline.ref}</div>
  <div>
    <span class="text-secondary">{t('pipeline.duration')}:</span>
    {formatDuration(pipeline.started_at, pipeline.finished_at)}
  </div>
</div>

{#if pipeline.stages?.length > 0}
  <PipelineFlow
    {pipeline}
    {approvedJobs}
    onOpenJobLog={onOpenJobLog}
    onPlayJob={onPlayJob}
    onApproveJob={onApproveJob}
    onRerunJob={onRerunJob}
  />
{:else}
  <p class="text-secondary">{t('pipeline.select_detail')}</p>
{/if}

<ArtifactsPanel {owner} {repo} pipelineId={pipeline.id} status={pipeline.status} />

<style>
  h2 {
    font-size: 20px;
    margin: 0;
  }

  .detail-header {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 16px;
  }

  .detail-actions {
    margin-left: auto;
    display: flex;
    gap: 8px;
  }

  .btn-outline {
    padding: 4px 12px;
    background: none;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text-primary);
    font-size: 12px;
    cursor: pointer;
  }

  .btn-outline:hover { background: var(--bg-hover); }

  .btn-danger { border-color: var(--red-dim); color: var(--red); }

  .detail-info {
    display: flex;
    gap: 24px;
    font-size: 13px;
    margin-bottom: 20px;
    padding: 12px 16px;
    background: var(--bg-primary);
    border-radius: var(--radius);
    flex-wrap: wrap;
  }

  .detail-info code {
    font-size: 12px;
    background: var(--bg-tertiary);
    padding: 1px 6px;
    border-radius: 3px;
  }
</style>
