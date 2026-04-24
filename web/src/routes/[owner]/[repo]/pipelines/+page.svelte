<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import PipelineBadge from '$lib/components/PipelineBadge.svelte';
  import { pipelines } from '$lib/api/client';

  let owner = $derived($page.params.owner);
  let repo = $derived($page.params.repo);
  let pipelineList = $state<any[]>([]);
  let selectedPipeline = $state<any>(null);
  let loading = $state(true);
  let error = $state('');

  $effect(() => { loadPipelines(); });

  async function loadPipelines() {
    try {
      loading = true;
      const pipeResult = await pipelines.list(owner, repo);
      pipelineList = pipeResult.data;
      if (pipelineList.length > 0) {
        selectedPipeline = await pipelines.get(owner, repo, pipelineList[0].id);
      }
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function selectPipeline(id: number) {
    try {
      selectedPipeline = await pipelines.get(owner, repo, id);
    } catch (e: any) {
      error = e.message;
    }
  }

  async function handleRetry(id: number) {
    try {
      await pipelines.retry(owner, repo, id);
      await loadPipelines();
    } catch (e: any) {
      error = e.message;
    }
  }

  async function handleCancel(id: number) {
    try {
      await pipelines.cancel(owner, repo, id);
      await loadPipelines();
    } catch (e: any) {
      error = e.message;
    }
  }

  function formatDate(dateStr: string) {
    return new Date(dateStr).toLocaleDateString('en-US', { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
  }

  function duration(start: string, end?: string) {
    if (!start) return '-';
    const s = new Date(start).getTime();
    const e = end ? new Date(end).getTime() : Date.now();
    const sec = Math.floor((e - s) / 1000);
    if (sec < 60) return sec + 's';
    if (sec < 3600) return Math.floor(sec / 60) + 'm ' + (sec % 60) + 's';
    return Math.floor(sec / 3600) + 'h ' + Math.floor((sec % 3600) / 60) + 'm';
  }
</script>

<svelte:head>
  <title>CI/CD · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="repo-page">
  <RepoHeader {owner} {repo} activeTab="pipelines" />

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="text-secondary">Loading pipelines...</p>
  {:else if pipelineList.length === 0}
    <div class="empty">
      <p>No pipelines yet.</p>
      <p class="text-secondary">Push a commit with <code>.ironforge-ci.yml</code> to trigger a pipeline.</p>
    </div>
  {:else}
    <div class="pipeline-layout">
      <!-- Pipeline list -->
      <div class="pipeline-list">
        <h3>Pipelines</h3>
        {#each pipelineList as p}
          <div class="pipeline-item" class:active={selectedPipeline?.id === p.id} onclick={() => selectPipeline(p.id)} role="button" tabindex="0">
            <PipelineBadge status={p.status} />
            <div class="pipeline-info">
              <div class="pipeline-msg truncate">{p.commit_message?.split('\n')[0] || '#' + p.id}</div>
              <div class="pipeline-meta">
                <span class="mono">{p.commit_sha?.slice(0, 7)}</span>
                <span>{duration(p.started_at, p.finished_at)}</span>
              </div>
            </div>
          </div>
        {/each}
      </div>

      <!-- Pipeline detail -->
      <div class="pipeline-detail">
        {#if selectedPipeline}
          <div class="detail-header">
            <h2>Pipeline #{selectedPipeline.id}</h2>
            <PipelineBadge status={selectedPipeline.status} />
            <div class="detail-actions">
              {#if selectedPipeline.status === 'failed'}
                <button class="btn-outline" onclick={() => handleRetry(selectedPipeline.id)}>↻ Retry</button>
              {/if}
              {#if selectedPipeline.status === 'running'}
                <button class="btn-outline btn-danger" onclick={() => handleCancel(selectedPipeline.id)}>✗ Cancel</button>
              {/if}
            </div>
          </div>

          <div class="detail-info">
            <div><span class="text-secondary">Commit:</span> <code>{selectedPipeline.commit_sha?.slice(0, 7)}</code></div>
            <div><span class="text-secondary">Branch:</span> {selectedPipeline.ref}</div>
            <div><span class="text-secondary">Duration:</span> {duration(selectedPipeline.started_at, selectedPipeline.finished_at)}</div>
          </div>

          <!-- Stages -->
          {#if selectedPipeline.stages}
            <div class="stages">
              {#each selectedPipeline.stages as stage}
                <div class="stage">
                  <div class="stage-header">
                    <PipelineBadge status={stage.status} />
                    <span class="stage-name">{stage.name}</span>
                  </div>
                  <div class="jobs">
                    {#each stage.jobs as job}
                      <div class="job" class:running={job.status === 'running'} class:success={job.status === 'success'} class:failed={job.status === 'failed'}>
                        <PipelineBadge status={job.status} />
                        <span class="job-name">{job.name}</span>
                        <span class="job-duration">{duration(job.started_at, job.finished_at)}</span>
                      </div>
                    {/each}
                  </div>
                </div>
              {/each}
            </div>
          {/if}
        {:else}
          <p class="text-secondary">Select a pipeline to view details.</p>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .repo-page { max-width: 1100px; margin: 0 auto; padding: 24px; }

  .error-banner { background: rgba(248, 81, 73, 0.1); border: 1px solid var(--red-dim); color: var(--red); border-radius: var(--radius); padding: 10px 14px; font-size: 13px; }
  .empty { text-align: center; padding: 48px; color: var(--text-secondary); }
  .empty code { background: var(--bg-tertiary); padding: 2px 6px; border-radius: 3px; }

  .pipeline-layout {
    display: grid;
    grid-template-columns: 320px 1fr;
    gap: 24px;
  }

  @media (max-width: 768px) {
    .pipeline-layout { grid-template-columns: 1fr; }
  }

  .pipeline-list {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }

  h3 { padding: 12px 16px; border-bottom: 1px solid var(--border); font-size: 14px; }

  .pipeline-item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border-light);
    cursor: pointer;
  }
  .pipeline-item:last-child { border-bottom: none; }
  .pipeline-item:hover { background: var(--bg-hover); }
  .pipeline-item.active { background: var(--bg-tertiary); border-left: 3px solid var(--accent); }

  .pipeline-info { flex: 1; min-width: 0; }
  .pipeline-msg { font-size: 13px; font-weight: 500; }
  .pipeline-meta { font-size: 11px; color: var(--text-muted); margin-top: 2px; display: flex; gap: 8px; }

  .pipeline-detail {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 24px;
  }

  .detail-header {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 16px;
  }
  h2 { font-size: 20px; }

  .detail-actions { margin-left: auto; display: flex; gap: 8px; }

  .btn-outline {
    padding: 4px 12px; background: none; border: 1px solid var(--border);
    border-radius: var(--radius); color: var(--text-primary); font-size: 12px; cursor: pointer;
  }
  .btn-outline:hover { background: var(--bg-hover); }
  .btn-danger { border-color: var(--red-dim); color: var(--red); }

  .detail-info {
    display: flex; gap: 24px; font-size: 13px; margin-bottom: 24px;
    padding: 12px 16px; background: var(--bg-primary); border-radius: var(--radius);
  }
  .detail-info code { font-size: 12px; background: var(--bg-tertiary); padding: 1px 6px; border-radius: 3px; }

  .stages { display: flex; flex-direction: column; gap: 12px; }

  .stage { border: 1px solid var(--border); border-radius: var(--radius); overflow: hidden; }
  .stage-header { display: flex; align-items: center; gap: 8px; padding: 8px 12px; background: var(--bg-tertiary); }
  .stage-name { font-weight: 600; font-size: 13px; }

  .jobs { padding: 4px 0; }

  .job {
    display: flex; align-items: center; gap: 8px;
    padding: 8px 16px; font-size: 13px;
    border-bottom: 1px solid var(--border-light);
  }
  .job:last-child { border-bottom: none; }
  .job-name { flex: 1; }
  .job-duration { font-size: 12px; color: var(--text-muted); font-family: var(--font-mono); }
</style>
