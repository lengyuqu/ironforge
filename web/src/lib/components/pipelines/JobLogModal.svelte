<script lang="ts">
  // Job log viewer modal — shows a job's log with live WebSocket streaming
  // status (Live / Reconnecting / Closed / error) and the job's if-condition.
  // Self-contained: owns the WebSocket connection and auto-scrolls.
  import { onDestroy } from 'svelte';
  import PipelineBadge from '$lib/components/PipelineBadge.svelte';
  import { connectJobLogWebSocket, disconnectJobLogWebSocket } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import type { PipelineJob } from '$lib/types/entities';

  interface Props {
    job: PipelineJob;
    /** Set when the job fetch itself failed; shows the error instead of a log. */
    initialError?: string;
    onClose: () => void;
  }

  let { job, initialError = '', onClose }: Props = $props();

  const t = createT();

  // Snapshot the props once at mount: this modal is remounted per job, so
  // reacting to later prop changes is unnecessary.
  const initialLog = job.log || '';
  const failedToLoad = initialError !== '';

  let logContent = $state(failedToLoad ? '' : initialLog);
  let logStreamStatus = $state<'idle' | 'connected' | 'reconnecting' | 'closed' | 'error'>(failedToLoad ? 'error' : 'idle');
  let logStreamError = $state(failedToLoad ? initialError : '');
  let logContentEl = $state<HTMLPreElement | null>(null);

  function appendLogChunk(jobId: number, chunk: string) {
    if (!chunk || job.id !== jobId) return;
    logContent += chunk;
    requestAnimationFrame(() => {
      if (logContentEl) {
        logContentEl.scrollTop = logContentEl.scrollHeight;
      }
    });
  }

  function startLogStream() {
    logStreamStatus = 'idle';
    logStreamError = '';
    // Resume from the lines already rendered so the server replays
    // only the log output the client has not seen yet (Q6.1).
    const since = logContent ? logContent.split('\n').length : 0;
    connectJobLogWebSocket(
      job.id,
      (chunk) => appendLogChunk(job.id, chunk),
      (status) => {
        logStreamStatus = status;
      },
      () => {
        logStreamStatus = 'error';
        logStreamError = 'Live log connection failed';
      },
      since,
    );
  }

  function closeByKey(e: KeyboardEvent) {
    if (e.key === 'Escape' || e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      close();
    }
  }

  function close() {
    disconnectJobLogWebSocket();
    logStreamStatus = 'idle';
    logStreamError = '';
    onClose();
  }

  if (!failedToLoad) startLogStream();

  onDestroy(() => {
    disconnectJobLogWebSocket();
  });
</script>

<div class="log-overlay-wrap">
  <button
    type="button"
    class="log-overlay"
    onclick={close}
    aria-label={t('common.cancel')}
  ></button>
  <div
    class="log-modal"
    role="dialog"
    aria-modal="true"
    aria-labelledby="pipeline-log-title"
    tabindex="-1"
    onkeydown={closeByKey}
  >
    <div class="log-header">
      <div>
        <strong id="pipeline-log-title">{job.name || 'Job Log'}</strong>
        <PipelineBadge status={job.status} />
        {#if logStreamStatus === 'connected'}
          <span class="log-live connected">Live</span>
        {:else if logStreamStatus === 'reconnecting'}
          <span class="log-live">Reconnecting…</span>
        {:else if logStreamStatus === 'closed'}
          <span class="log-live">Closed</span>
        {:else if logStreamStatus === 'error'}
          <span class="log-live error">{logStreamError || 'Offline'}</span>
        {/if}
      </div>
      <button class="btn-close" onclick={close}>✕</button>
    </div>
    {#if job.if_condition}
      <div class="log-condition">
        <span>{job.status === 'skipped' ? t('pipeline.condition_skipped') : t('pipeline.condition')}</span>
        <code>{job.if_condition}</code>
      </div>
    {/if}
    <pre class="log-content" bind:this={logContentEl}><code>{logContent || '(no log output)'}</code></pre>
  </div>
</div>

<style>
  .log-overlay-wrap {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 40px;
  }

  .log-overlay {
    position: absolute;
    inset: 0;
    z-index: 1;
    background: rgba(0, 0, 0, 0.5);
    border: none;
    margin: 0;
    padding: 0;
    cursor: default;
  }

  .log-modal {
    position: relative;
    z-index: 2;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: var(--radius-lg);
    width: 100%;
    max-width: 800px;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.2);
  }

  .log-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
    font-size: 14px;
  }
  .log-header > div { display: flex; align-items: center; gap: 8px; }

  .log-condition {
    align-items: baseline;
    background: var(--bg-secondary);
    border-bottom: 1px solid var(--border);
    color: var(--text-secondary);
    display: flex;
    font-size: 12px;
    gap: 10px;
    padding: 8px 16px;
  }
  .log-condition code { color: var(--text-primary); overflow-wrap: anywhere; }

  .log-live {
    font-size: 11px;
    color: var(--text-muted);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 2px 8px;
  }
  .log-live.connected {
    color: var(--green);
    border-color: color-mix(in srgb, var(--green) 45%, var(--border));
  }
  .log-live.error {
    color: var(--red);
    border-color: color-mix(in srgb, var(--red) 45%, var(--border));
  }

  .btn-close {
    background: none;
    border: none;
    font-size: 18px;
    cursor: pointer;
    color: var(--text-muted);
    padding: 4px 8px;
    border-radius: 4px;
  }
  .btn-close:hover { background: var(--bg-hover); color: var(--text-primary); }

  .log-content {
    overflow: auto;
    padding: 16px;
    margin: 0;
    font-size: 12px;
    line-height: 1.5;
    background: #1a1a2e;
    color: #e0e0e0;
    border-radius: 0 0 var(--radius-lg) var(--radius-lg);
    white-space: pre-wrap;
    word-break: break-all;
    max-height: 60vh;
  }
</style>
