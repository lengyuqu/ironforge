<script lang="ts">
  import { artifacts } from '$lib/api/client.svelte';
  import { downloadApiFile } from '$lib/api/_base.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT } from '$lib/i18n';
  import type { Artifact } from '$lib/types/entities';

  const t = createT();

  let {
    owner,
    repo,
    pipelineId,
    status,
  }: {
    owner: string;
    repo: string;
    pipelineId: number;
    status: string;
  } = $props();

  const TERMINAL_STATUSES = ['success', 'failed', 'cancelled'];

  let list = $state<Artifact[]>([]);
  let loading = $state(false);
  let error = $state('');
  let downloadingId = $state<number | null>(null);

  const isTerminal = $derived(TERMINAL_STATUSES.includes(status));

  // Reload on pipeline switch; reload once when the pipeline transitions to a
  // terminal state (artifacts are uploaded when jobs finish).
  let wasTerminal = $state(false);
  $effect(() => {
    const id = pipelineId;
    const terminal = isTerminal;
    const firstLoad = !wasTerminal && !loading;
    if (firstLoad || (terminal && !wasTerminal)) {
      loadArtifacts(id);
    }
    wasTerminal = terminal;
  });

  async function loadArtifacts(id: number) {
    loading = true;
    error = '';
    try {
      list = await artifacts.listByPipeline(owner, repo, id);
    } catch (e) {
      error = toErrorMessage(e, t('pipeline.artifacts_load_failed', 'Failed to load artifacts'));
    } finally {
      loading = false;
    }
  }

  async function handleDownload(a: Artifact) {
    downloadingId = a.id;
    try {
      await downloadApiFile(`/artifacts/${a.id}`, a.name);
      toast.success(t('pipeline.artifact_download_started', 'Download started'));
    } catch (e) {
      toast.error(toErrorMessage(e, t('pipeline.artifact_download_failed', 'Download failed')));
    } finally {
      downloadingId = null;
    }
  }

  function formatBytes(bytes: number): string {
    if (bytes <= 0) return '0 B';
    const units = ['B', 'KB', 'MB', 'GB'];
    let value = bytes;
    let i = 0;
    while (value >= 1024 && i < units.length - 1) {
      value /= 1024;
      i++;
    }
    return `${value >= 10 || i === 0 ? Math.round(value) : value.toFixed(1)} ${units[i]}`;
  }

  function relativeTime(iso: string): string {
    const ms = Date.now() - new Date(iso).getTime();
    if (ms < 0) return t('common.just_now', 'just now');
    const minutes = Math.floor(ms / 60_000);
    if (minutes < 1) return t('common.just_now', 'just now');
    if (minutes < 60) return `${minutes}m`;
    const hours = Math.floor(minutes / 60);
    if (hours < 24) return `${hours}h`;
    return `${Math.floor(hours / 24)}d`;
  }

  function expiryInfo(a: Artifact): { label: string; expired: boolean } | null {
    if (!a.expires_at) return null;
    const remaining = new Date(a.expires_at).getTime() - Date.now();
    if (remaining <= 0) {
      return { label: t('pipeline.artifact_expired', 'Expired'), expired: true };
    }
    const days = Math.ceil(remaining / 86_400_000);
    return { label: `${t('pipeline.artifact_expires_in', 'Expires in')} ${days}d`, expired: false };
  }
</script>

<section class="artifacts-panel">
  <h3>{t('pipeline.artifacts', 'Artifacts')}</h3>

  {#if error}
    <div class="panel-error">{error}</div>
  {/if}

  {#if loading}
    <p class="muted">{t('common.loading', 'Loading…')}</p>
  {:else if list.length === 0}
    <p class="muted">{t('pipeline.no_artifacts', 'No artifacts for this pipeline.')}</p>
  {:else}
    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            <th>{t('pipeline.artifact_name', 'Name')}</th>
            <th>{t('pipeline.artifact_size', 'Size')}</th>
            <th>{t('pipeline.artifact_uploaded', 'Uploaded')}</th>
            <th>{t('pipeline.artifact_expiry', 'Expiry')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each list as a (a.id)}
            {@const expiry = expiryInfo(a)}
            <tr>
              <td class="name-cell" title={a.file_path}>{a.name}</td>
              <td class="size-cell">{formatBytes(a.size)}</td>
              <td class="time-cell" title={a.created_at}>{relativeTime(a.created_at)}</td>
              <td>
                {#if expiry}
                  <span class="expiry" class:expired={expiry.expired}>{expiry.label}</span>
                {:else}
                  <span class="muted">—</span>
                {/if}
              </td>
              <td class="actions">
                <button
                  class="download-btn"
                  disabled={downloadingId === a.id}
                  onclick={() => handleDownload(a)}
                >{downloadingId === a.id ? t('common.loading', '…') : t('common.download', 'Download')}</button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>

<style>
  .artifacts-panel {
    margin-top: 16px;
    padding: 16px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }

  h3 { margin: 0 0 12px; font-size: 15px; }

  .muted { color: var(--text-secondary); font-size: 13px; }
  .panel-error {
    color: #f85149; background: rgba(248, 81, 73, 0.1);
    padding: 8px 12px; border-radius: 6px; margin-bottom: 12px; font-size: 13px;
  }

  .table-wrap { overflow-x: auto; }
  table { width: 100%; border-collapse: collapse; font-size: 13px; }
  th {
    text-align: left; padding: 6px 8px; color: var(--text-secondary);
    font-weight: 600; border-bottom: 1px solid var(--border); white-space: nowrap;
  }
  td { padding: 8px; border-bottom: 1px solid var(--border); color: var(--text-primary); }

  .name-cell { max-width: 280px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .size-cell { white-space: nowrap; color: var(--text-secondary); }
  .time-cell { white-space: nowrap; color: var(--text-secondary); }

  .expiry {
    font-size: 12px; padding: 2px 8px; border-radius: 10px;
    border: 1px solid var(--border); color: var(--text-secondary); white-space: nowrap;
  }
  .expiry.expired { border-color: var(--red-dim); color: var(--red); }

  .actions { text-align: right; }
  .download-btn {
    padding: 4px 12px; background: none; border: 1px solid var(--border);
    border-radius: var(--radius); color: var(--text-primary);
    font-size: 12px; cursor: pointer;
  }
  .download-btn:hover:not(:disabled) { background: var(--bg-hover); }
  .download-btn:disabled { opacity: 0.6; cursor: not-allowed; }
</style>
