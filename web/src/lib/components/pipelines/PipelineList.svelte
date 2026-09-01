<script lang="ts">
  // Pipeline sidebar list — selectable pipeline rows with status badge,
  // commit subject and duration.
  import { createT } from '$lib/i18n';
  import PipelineBadge from '$lib/components/PipelineBadge.svelte';
  import { formatDuration } from '$lib/utils/pipelineStatus';
  import type { Pipeline } from '$lib/types/entities';

  interface Props {
    pipelines: Pipeline[];
    selectedId?: number;
    onSelect: (id: number) => void;
  }

  let { pipelines, selectedId, onSelect }: Props = $props();

  const t = createT();

  function selectByKey(e: KeyboardEvent, id: number) {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      onSelect(id);
    }
  }
</script>

<h3>{t('repo.tabs.pipelines')}</h3>
<div class="list-scroll">
  {#each pipelines as p (p.id)}
    <div
      class="pipeline-item"
      class:active={selectedId === p.id}
      onclick={() => onSelect(p.id)}
      onkeydown={(e) => selectByKey(e, p.id)}
      role="button"
      tabindex="0"
    >
      <PipelineBadge status={p.status} />
      <div class="pipeline-info">
        <div class="pipeline-msg truncate">#{p.id} · {p.ref_name}</div>
        <div class="pipeline-meta">
          <span class="mono">{p.commit_sha?.slice(0, 7)}</span>
          <span>{formatDuration(p.started_at, p.finished_at)}</span>
        </div>
      </div>
    </div>
  {/each}
</div>

<style>
  h3 {
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
    font-size: 14px;
    margin: 0;
  }

  .list-scroll { overflow-y: auto; max-height: 70vh; }

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
  .pipeline-item.active {
    background: var(--bg-tertiary);
    border-left: 3px solid var(--accent);
  }

  .pipeline-info { flex: 1; min-width: 0; }
  .pipeline-msg { font-size: 13px; font-weight: 500; }
  .pipeline-meta {
    font-size: 11px;
    color: var(--text-muted);
    margin-top: 2px;
    display: flex;
    gap: 8px;
  }
</style>
