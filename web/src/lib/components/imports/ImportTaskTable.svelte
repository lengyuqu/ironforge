<script lang="ts">
  import { imports, type ImportTask } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT, formatDateTime } from '$lib/i18n';

  const t = createT();

  let {
    tasks,
    onDeleted,
  }: {
    tasks: ImportTask[];
    onDeleted: (id: number) => void;
  } = $props();

  let deletingId = $state<number | null>(null);

  function taskHref(task: ImportTask): string {
    return `/${encodeURIComponent(task.target_owner)}/${encodeURIComponent(task.target_name)}`;
  }

  async function deleteImport(task: ImportTask) {
    if (!confirm('Cancel and delete this import task?')) return;
    deletingId = task.id;
    try {
      await imports.remove(task.id);
      toast.success('Import deleted');
      onDeleted(task.id);
    } catch (e) {
      toast.error(toErrorMessage(e, t('errors.delete_failed')));
    } finally {
      deletingId = null;
    }
  }
</script>

<div class="table-wrap">
  <table>
    <thead>
      <tr>
        <th>Source</th>
        <th>Target</th>
        <th>Status</th>
        <th>Progress</th>
        <th>Updated</th>
        <th></th>
      </tr>
    </thead>
    <tbody>
      {#each tasks as task}
        <tr>
          <td>
            <div class="source">
              <span class="platform">{task.platform}</span>
              <a href={task.source_url} target="_blank" rel="noreferrer">{task.source_url}</a>
            </div>
            {#if task.error}
              <div class="task-error">{task.error}</div>
            {/if}
          </td>
          <td><a href={taskHref(task)}>{task.target_owner}/{task.target_name}</a></td>
          <td><span class="status">{task.status}</span></td>
          <td>
            {task.progress}%
            {#if task.stage}
              <div class="muted small">{task.stage}</div>
            {/if}
          </td>
          <td>{formatDateTime(task.updated_at || task.created_at)}</td>
          <td class="row-actions">
            <button class="btn-danger" type="button" disabled={deletingId === task.id} onclick={() => deleteImport(task)}>Delete</button>
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

<style>
  .table-wrap {
    overflow-x: auto;
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
  }

  th,
  td {
    padding: 10px 8px;
    border-bottom: 1px solid var(--border);
    text-align: left;
    vertical-align: top;
  }

  th {
    color: var(--text-secondary);
    font-weight: 600;
  }

  .source {
    display: grid;
    gap: 4px;
    min-width: 260px;
  }

  .platform,
  .status {
    width: fit-content;
    padding: 2px 6px;
    border: 1px solid var(--border);
    border-radius: 999px;
    color: var(--text-secondary);
    font-size: 11px;
    text-transform: uppercase;
  }

  .task-error {
    margin-top: 6px;
    color: #f85149;
  }

  .muted {
    color: var(--text-secondary);
  }

  .small {
    margin-top: 4px;
    font-size: 12px;
  }

  .row-actions {
    text-align: right;
  }

  .btn-danger {
    background: rgba(248, 81, 73, 0.12);
    color: #f85149;
    border: 0;
    border-radius: 6px;
    padding: 8px 12px;
    cursor: pointer;
    font-weight: 600;
  }

  button:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
</style>
