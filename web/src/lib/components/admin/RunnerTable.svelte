<script lang="ts">
  // Runners table — pure presentation: renders runner rows and delegates
  // delete intents to the parent via callbacks.
  import { createT } from '$lib/i18n';
  import type { RunnerListItem } from '$lib/api/client.svelte';

  interface Props {
    items: RunnerListItem[];
    onDelete: (runner: RunnerListItem) => void;
  }

  let { items, onDelete }: Props = $props();

  const t = createT();
</script>

<div class="table-wrap">
  <table class="runners-table">
    <thead>
      <tr>
        <th>{t('admin.runners.name')}</th>
        <th>{t('admin.runners.status')}</th>
        <th>{t('admin.runners.labels')}</th>
        <th>{t('admin.runners.version')}</th>
        <th>{t('admin.runners.last_seen')}</th>
        <th></th>
      </tr>
    </thead>
    <tbody>
      {#each items as runner (runner.id)}
        <tr>
          <td class="name">{runner.name}</td>
          <td>
            <span class="badge" class:online={runner.status === 'online'}>{runner.status}</span>
          </td>
          <td>
            <div class="labels">
              {#each runner.labels as label (label)}
                <span>{label}</span>
              {/each}
            </div>
          </td>
          <td class="muted">{runner.version || '-'}</td>
          <td class="muted">
            {runner.last_seen ? new Date(runner.last_seen).toLocaleString() : t('common.never')}
          </td>
          <td class="actions">
            <button class="btn-danger" onclick={() => onDelete(runner)}>
              {t('common.delete')}
            </button>
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

  .runners-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.9rem;
  }

  .runners-table th {
    text-align: left;
    padding: 0.6rem 0.75rem;
    border-bottom: 2px solid var(--border);
    color: var(--text-secondary);
    font-weight: 600;
  }

  .runners-table td {
    padding: 0.6rem 0.75rem;
    border-bottom: 1px solid var(--border);
    color: var(--text-primary);
    vertical-align: top;
  }

  .runners-table tr:hover td {
    background: var(--bg-hover);
  }

  .name {
    font-weight: 600;
  }

  .muted {
    color: var(--text-secondary);
  }

  .labels {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
  }

  .labels span {
    padding: 0.1rem 0.4rem;
    border: 1px solid var(--border);
    border-radius: 999px;
    color: var(--text-secondary);
    font-size: 0.8rem;
  }

  .actions {
    text-align: right;
  }

  .badge {
    display: inline-block;
    padding: 0.1rem 0.45rem;
    border-radius: 999px;
    font-size: 0.8rem;
    background: rgba(139, 148, 158, 0.15);
    color: var(--text-secondary);
    border: 1px solid var(--border);
  }

  .badge.online {
    background: rgba(63, 185, 80, 0.15);
    color: #3fb950;
    border-color: #3fb950;
  }

  .btn-danger {
    background: rgba(248, 81, 73, 0.15);
    border: 1px solid #f85149;
    color: #f85149;
    border-radius: 4px;
    padding: 0.25rem 0.6rem;
    font-size: 0.8rem;
    cursor: pointer;
  }

  .btn-danger:hover {
    background: rgba(248, 81, 73, 0.25);
  }
</style>
