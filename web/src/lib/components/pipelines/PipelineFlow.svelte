<script lang="ts">
  // Pipeline flow visualization — vertical stage list with connector arrows
  // and clickable job cards (open log / play manual / approve environment).
  import { createT } from '$lib/i18n';
  import { formatDuration, statusIcon, statusColor } from '$lib/utils/pipelineStatus';
  import type { PipelineDetail } from '$lib/types/entities';

  interface Props {
    pipeline: PipelineDetail;
    /** Job ids whose approval has already been recorded locally. */
    approvedJobs: number[];
    onOpenJobLog: (jobId: number) => void;
    onPlayJob: (jobId: number) => void;
    onApproveJob: (jobId: number) => void;
  }

  let { pipeline, approvedJobs, onOpenJobLog, onPlayJob, onApproveJob }: Props = $props();

  const t = createT();

  function openJobByKey(e: KeyboardEvent, jobId: number) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      onOpenJobLog(jobId);
    }
  }
</script>

<div class="pipeline-flow">
  {#each pipeline.stages as stage, si (stage.id)}
    <div class="flow-stage">
      <!-- Stage header -->
      <div class="stage-label">
        <span class="stage-dot" style="background:{statusColor(stage.status)}"></span>
        <span class="stage-name">{stage.name}</span>
        <span class="stage-dur">{formatDuration(stage.started_at, stage.finished_at)}</span>
      </div>

      <!-- Connector arrow between stages -->
      {#if si < pipeline.stages.length - 1}
        <div class="stage-connector">
          <svg width="16" height="24" viewBox="0 0 16 24">
            <line x1="8" y1="0" x2="8" y2="20" stroke="var(--border)" stroke-width="2"/>
            <polyline points="2,16 8,22 14,16" fill="none" stroke="var(--border)" stroke-width="2"/>
          </svg>
        </div>
      {/if}

      <!-- Jobs in this stage -->
      <div class="jobs-flow">
        {#each stage.jobs as job (job.id)}
          <div
            class="job-card"
            class:running={job.status === 'running'}
            class:failed={job.status === 'failed'}
            onclick={() => onOpenJobLog(job.id)}
            onkeydown={(e) => openJobByKey(e, job.id)}
            role="button"
            tabindex="0"
          >
            <div class="job-status-icon" style="color:{statusColor(job.status)}">
              {#if job.status === 'running'}
                <span class="spin">{statusIcon(job.status)}</span>
              {:else}
                {statusIcon(job.status)}
              {/if}
            </div>
            <div class="job-body">
              <span class="job-name">{job.name}</span>
              {#if job.environment_name}<span class="environment-name">🚀 {job.environment_name}</span>{/if}
              {#if job.if_condition}
                <span
                  class="job-condition"
                  class:condition-skipped={job.status === 'skipped'}
                  title={`${job.status === 'skipped' ? t('pipeline.condition_skipped') : t('pipeline.condition')}: ${job.if_condition}`}
                >if</span>
              {/if}
              <span class="job-dur">{formatDuration(job.started_at, job.finished_at)}</span>
            </div>
            {#if job.exit_code !== null && job.exit_code !== undefined}
              <span class="exit-code">{job.exit_code}</span>
            {/if}
            {#if job.status === 'manual'}
              <button class="play-job" onclick={(event) => { event.stopPropagation(); onPlayJob(job.id); }}>{t('pipeline.play_manual')}</button>
            {/if}
            {#if job.status === 'waiting_approval'}
              <button
                class="play-job"
                disabled={approvedJobs.includes(job.id)}
                onclick={(event) => { event.stopPropagation(); onApproveJob(job.id); }}
              >{approvedJobs.includes(job.id) ? t('pipeline.approval_recorded') : t('pipeline.approve_environment')}</button>
            {/if}
          </div>
        {/each}
      </div>
    </div>
  {/each}
</div>

<style>
  .pipeline-flow {
    display: flex;
    flex-direction: column;
    gap: 0;
    padding: 8px 0;
  }

  .flow-stage { position: relative; }

  .stage-label {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    background: var(--bg-tertiary);
    border-radius: 6px 6px 0 0;
    border: 1px solid var(--border);
    border-bottom: none;
  }
  .stage-dot { width: 8px; height: 8px; border-radius: 50%; display: inline-block; }
  .stage-name {
    flex: 1;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    font-size: 11px;
  }
  .stage-dur {
    font-size: 11px;
    color: var(--text-muted);
    font-family: var(--font-mono);
  }

  .stage-connector {
    display: flex;
    justify-content: center;
    padding: 2px 0;
    height: 28px;
  }

  .jobs-flow {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 4px;
    border: 1px solid var(--border);
    border-radius: 0 0 6px 6px;
    background: var(--bg-primary);
  }

  .job-card {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    border-radius: 4px;
    border: none;
    background: transparent;
    cursor: pointer;
    text-align: left;
    width: 100%;
    font-size: 13px;
    transition: background 0.15s;
  }
  .job-card:hover { background: var(--bg-hover); }
  .job-card.running { background: rgba(88, 166, 255, 0.08); }
  .job-card.failed { background: rgba(248, 81, 73, 0.06); }

  .job-status-icon {
    width: 20px;
    text-align: center;
    font-size: 14px;
    flex-shrink: 0;
  }

  .job-body {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .job-name {
    flex: 1;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .environment-name { color: var(--text-secondary); font-size: 11px; white-space: nowrap; }
  .job-condition {
    border: 1px solid var(--border);
    border-radius: 999px;
    color: var(--text-secondary);
    cursor: help;
    font-family: var(--font-mono);
    font-size: 10px;
    line-height: 16px;
    padding: 0 5px;
  }
  .job-condition.condition-skipped { border-color: var(--text-muted); color: var(--text-muted); }
  .job-dur {
    font-size: 11px;
    color: var(--text-muted);
    font-family: var(--font-mono);
    white-space: nowrap;
  }

  .exit-code {
    font-size: 11px;
    font-family: var(--font-mono);
    padding: 1px 6px;
    border-radius: 3px;
    background: var(--bg-tertiary);
    color: var(--text-muted);
  }

  .play-job {
    padding: 3px 9px;
    border: 1px solid var(--accent);
    border-radius: 4px;
    color: var(--accent);
    background: transparent;
    cursor: pointer;
    font-size: 11px;
  }
  .play-job:hover { background: rgba(88, 166, 255, 0.1); }
  .play-job:disabled { opacity: 0.6; cursor: default; }

  .spin { display: inline-block; animation: spin 1.5s linear infinite; }
  @keyframes spin { from { transform: rotate(0deg); } to { transform: rotate(360deg); } }
</style>
