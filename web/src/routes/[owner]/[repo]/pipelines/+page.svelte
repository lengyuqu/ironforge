<script lang="ts">
  // Pipelines page — orchestrator. Loads the pipeline list and the selected
  // pipeline detail, auto-refreshes while running, and delegates UI to:
  //   PipelineList (sidebar) / PipelineFlow (stage-job visualization) /
  //   JobLogModal (live log viewer)
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import PipelineList from '$lib/components/pipelines/PipelineList.svelte';
  import JobLogModal from '$lib/components/pipelines/JobLogModal.svelte';
  import PipelineDetailPanel from '$lib/components/pipelines/PipelineDetailPanel.svelte';
  import { pipelines } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import { isRunning } from '$lib/utils/pipelineStatus';
  import type { Pipeline, PipelineDetail, PipelineDetailResponse, PipelineJob } from '$lib/types/entities';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  let pipelineList = $state<Pipeline[]>([]);
  let selectedPipeline = $state<PipelineDetail | null>(null);
  let loading = $state(true);
  let error = $state('');
  let selectedJob = $state<PipelineJob | null>(null);
  let approvedJobs = $state<number[]>([]);

  // Auto-refresh for running pipelines
  let refreshInterval: ReturnType<typeof setInterval> | null = null;

  function normalizePipelineDetail(detail: PipelineDetailResponse): PipelineDetail {
    return {
      ...detail.pipeline,
      ref: detail.pipeline.ref_name,
      stages: (detail.stages || [])
        .filter((entry) => entry.stage)
        .map((entry) => ({ ...entry.stage!, jobs: entry.jobs || [] })),
    };
  }

  $effect(() => {
    loadPipelines();

    // Start auto-refresh when a pipeline is running
    if (isRunning(selectedPipeline?.status ?? '')) {
      if (!refreshInterval) {
        refreshInterval = setInterval(() => {
          if (selectedPipeline) {
            pipelines.get(owner, repo, selectedPipeline.id).then((p) => {
              selectedPipeline = normalizePipelineDetail(p);
            });
          }
        }, 5000);
      }
    } else {
      if (refreshInterval) { clearInterval(refreshInterval); refreshInterval = null; }
    }

    // Unified cleanup: clear interval on re-run or component destroy
    return () => {
      if (refreshInterval) { clearInterval(refreshInterval); refreshInterval = null; }
    };
  });

  async function loadPipelines() {
    try {
      loading = true;
      const pipeResult = await pipelines.list(owner, repo);
      pipelineList = pipeResult.data;
      if (pipelineList.length > 0 && !selectedPipeline) {
        selectedPipeline = normalizePipelineDetail(await pipelines.get(owner, repo, pipelineList[0].id));
      }
    } catch (e: unknown) {
      error = toErrorMessage(e);
    } finally {
      loading = false;
    }
  }

  async function selectPipeline(id: number) {
    selectedJob = null;
    try {
      selectedPipeline = normalizePipelineDetail(await pipelines.get(owner, repo, id));
    } catch (e: unknown) {
      error = toErrorMessage(e);
    }
  }

  async function handleRetry(id: number) {
    try {
      await pipelines.retry(owner, repo, id);
      await loadPipelines();
    } catch (e: unknown) {
      error = toErrorMessage(e);
    }
  }

  async function handleCancel(id: number) {
    try {
      await pipelines.cancel(owner, repo, id);
      if (selectedPipeline) selectedPipeline.status = 'canceled';
    } catch (e: unknown) {
      error = toErrorMessage(e);
    }
  }

  async function handlePlay(jobId: number) {
    if (!selectedPipeline) return;
    try {
      await pipelines.play(owner, repo, selectedPipeline.id, jobId);
      selectedPipeline = normalizePipelineDetail(await pipelines.get(owner, repo, selectedPipeline.id));
    } catch (e: unknown) {
      error = toErrorMessage(e);
    }
  }

  async function handleRerun(jobId: number) {
    if (!selectedPipeline) return;
    try {
      await pipelines.rerun(owner, repo, selectedPipeline.id, jobId);
      selectedPipeline = normalizePipelineDetail(await pipelines.get(owner, repo, selectedPipeline.id));
    } catch (e: unknown) {
      error = toErrorMessage(e);
    }
  }

  async function handleApprove(jobId: number) {
    if (!selectedPipeline) return;
    try {
      const result = await pipelines.approve(owner, repo, selectedPipeline.id, jobId);
      if (!result.released && !approvedJobs.includes(jobId)) approvedJobs = [...approvedJobs, jobId];
      selectedPipeline = normalizePipelineDetail(await pipelines.get(owner, repo, selectedPipeline.id));
    } catch (e: unknown) {
      error = toErrorMessage(e);
    }
  }

  let jobLoadError = $state('');

  async function viewJobLog(jobId: number) {
    if (!selectedPipeline) return;
    try {
      jobLoadError = '';
      selectedJob = await pipelines.job(owner, repo, selectedPipeline.id, jobId);
    } catch (e: unknown) {
      // Open the panel anyway and show the load failure inside it.
      jobLoadError = t('errors.load_log', { message: toErrorMessage(e) });
      selectedJob = { id: jobId, stage_id: 0, name: `Job #${jobId}`, status: 'error' };
    }
  }

  function closeLog() {
    selectedJob = null;
  }
</script>

<svelte:head>
  <title>CI/CD · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-container">
  <RepoHeader {owner} {repo} activeTab="pipelines" />

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="text-secondary">{t('common.loading')}</p>
  {:else if pipelineList.length === 0}
    <div class="empty">
      <p>{t('pipeline.no_pipelines')}</p>
      <p class="text-secondary">{t('pipeline.hint', { file: t('pipeline.file') })}</p>
    </div>
  {:else}
    <div class="pipeline-layout">
      <!-- Pipeline list -->
      <div class="pipeline-list">
        <PipelineList pipelines={pipelineList} selectedId={selectedPipeline?.id} onSelect={selectPipeline} />
      </div>

      <!-- Pipeline detail -->
      <div class="pipeline-detail">
        {#if selectedPipeline}
          <PipelineDetailPanel
            {owner}
            {repo}
            pipeline={selectedPipeline}
            {approvedJobs}
            onRetry={handleRetry}
            onCancel={handleCancel}
            onOpenJobLog={viewJobLog}
            onPlayJob={handlePlay}
            onApproveJob={handleApprove}
            onRerunJob={handleRerun}
          />
        {:else}
          <p class="text-secondary">{t('pipeline.select_detail')}</p>
        {/if}
      </div>
    </div>
  {/if}
</div>

<!-- Log Viewer Modal -->
{#if selectedJob}
  <JobLogModal job={selectedJob} initialError={jobLoadError} onClose={closeLog} />
{/if}

<style>
  .empty { text-align: center; padding: 48px; color: var(--text-secondary); }

  .pipeline-layout {
    display: grid;
    grid-template-columns: 320px 1fr;
    gap: 24px;
  }
  @media (max-width: 900px) { .pipeline-layout { grid-template-columns: 1fr; } }

  .pipeline-list {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .pipeline-detail {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 24px;
  }

</style>
