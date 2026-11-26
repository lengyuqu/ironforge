<script lang="ts">
  // Pipelines page — orchestrator. Loads the pipeline list and the selected
  // pipeline detail, auto-refreshes while running, and delegates UI to:
  //   PipelineList (sidebar) / PipelineFlow (stage-job visualization) /
  //   JobLogModal (live log viewer)
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import Pagination from '$lib/components/Pagination.svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import PipelineList from '$lib/components/pipelines/PipelineList.svelte';
  import JobLogModal from '$lib/components/pipelines/JobLogModal.svelte';
  import PipelineDetailPanel from '$lib/components/pipelines/PipelineDetailPanel.svelte';
  import { pipelines } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import { isRunning } from '$lib/utils/pipelineStatus';
  import { createLatest } from '$lib/utils/latest';
  import { buildQueryHref, readPageParam } from '$lib/utils/urlQuery';
  import type { Pipeline, PipelineDetail, PipelineDetailResponse, PipelineJob } from '$lib/types/entities';

  const t = createT();
  const PER_PAGE = 20;

  let owner = $derived(page.params.owner!);
  let repo = $derived(page.params.repo!);
  let currentPage = $derived(readPageParam(page.url));
  let pipelineList = $state<Pipeline[]>([]);
  let total = $state(0);
  let selectedPipeline = $state<PipelineDetail | null>(null);
  let loading = $state(true);
  let error = $state('');
  let selectedJob = $state<PipelineJob | null>(null);
  let approvedJobs = $state<number[]>([]);

  const listLatest = createLatest();
  const detailLatest = createLatest();
  let loadedScope = '';

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
    const scope = `${owner}/${repo}`;
    if (scope !== loadedScope) {
      loadedScope = scope;
      selectedPipeline = null;
      selectedJob = null;
      approvedJobs = [];
      detailLatest.invalidate();
    }
    void loadPipelines();
  });

  const running = $derived(isRunning(selectedPipeline?.status ?? ''));
  const selectedId = $derived(selectedPipeline?.id);

  $effect(() => {
    if (!running || selectedId == null) return;
    const reqOwner = owner;
    const reqRepo = repo;
    const id = selectedId;
    const timer = setInterval(() => {
      pipelines
        .get(reqOwner, reqRepo, id)
        .then((p) => {
          if (selectedPipeline?.id === id && owner === reqOwner && repo === reqRepo) {
            selectedPipeline = normalizePipelineDetail(p);
          }
        })
        .catch(() => {});
    }, 5000);
    return () => clearInterval(timer);
  });

  async function loadPipelines() {
    const reqOwner = owner;
    const reqRepo = repo;
    const reqPage = currentPage;
    const isCurrent = listLatest.begin();
    try {
      loading = true;
      error = '';
      const pipeResult = await pipelines.list(reqOwner, reqRepo, reqPage, PER_PAGE);
      if (!isCurrent()) return;
      pipelineList = pipeResult.data;
      total = pipeResult.pagination?.total ?? pipeResult.data.length;
      if (reqPage > 1 && pipeResult.data.length === 0 && total > 0) {
        void goto(buildQueryHref(page.url, { page: null }), {
          replaceState: true,
          keepFocus: true,
          noScroll: true,
        });
        return;
      }
      if (pipelineList.length > 0 && !selectedPipeline) {
        const isDetailCurrent = detailLatest.begin();
        const detail = await pipelines.get(reqOwner, reqRepo, pipelineList[0].id);
        if (!isCurrent() || !isDetailCurrent()) return;
        selectedPipeline = normalizePipelineDetail(detail);
      }
    } catch (e: unknown) {
      if (!isCurrent()) return;
      error = toErrorMessage(e);
    } finally {
      if (isCurrent()) {
        loading = false;
      }
    }
  }

  function changePage(nextPage: number) {
    void goto(buildQueryHref(page.url, { page: nextPage === 1 ? null : nextPage }), {
      keepFocus: true,
    });
  }

  async function selectPipeline(id: number) {
    selectedJob = null;
    const isCurrent = detailLatest.begin();
    try {
      const detail = await pipelines.get(owner, repo, id);
      if (!isCurrent()) return;
      selectedPipeline = normalizePipelineDetail(detail);
    } catch (e: unknown) {
      if (!isCurrent()) return;
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
  <title>{t('repo.tabs.pipelines')} · {owner}/{repo} · IronForge</title>
</svelte:head>

{#if error}
  <ErrorBanner message={error} />
{/if}

{#if loading}
  <LoadingState />
{:else if pipelineList.length === 0}
  <EmptyState title={t('pipeline.no_pipelines')} description={t('pipeline.hint', { file: t('pipeline.file') })} />
{:else}
  <div class="pipeline-layout">
    <!-- Pipeline list -->
    <div class="pipeline-list surface">
      <PipelineList pipelines={pipelineList} selectedId={selectedPipeline?.id} onSelect={selectPipeline} />
      <Pagination
        {total}
        page={currentPage}
        perPage={PER_PAGE}
        siblingCount={1}
        onPageChange={changePage}
      />
    </div>

    <!-- Pipeline detail -->
    <div class="pipeline-detail surface">
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

<!-- Log Viewer Modal -->
{#if selectedJob}
  <JobLogModal job={selectedJob} initialError={jobLoadError} onClose={closeLog} />
{/if}

<style>
  .pipeline-layout {
    display: grid;
    grid-template-columns: 320px 1fr;
    gap: 24px;
  }
  @media (max-width: 900px) { .pipeline-layout { grid-template-columns: 1fr; } }

  .pipeline-list {
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .pipeline-list :global(.pagination) {
    padding: 10px 12px;
    border-top: 1px solid var(--border);
  }

  .pipeline-detail {
    padding: 24px;
  }

</style>
