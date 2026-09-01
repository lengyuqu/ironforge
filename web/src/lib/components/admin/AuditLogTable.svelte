<script lang="ts">
  import { createT, formatDate, formatDateTime } from '$lib/i18n';
  import type { AuditLogEntry } from '$lib/api/client.svelte';

  const t = createT();

  let {
    logs,
    page,
    totalPages,
    onOpenDetail,
    onPrevPage,
    onNextPage,
  }: {
    logs: AuditLogEntry[];
    page: number;
    totalPages: number;
    onOpenDetail: (log: AuditLogEntry) => void;
    onPrevPage: () => void;
    onNextPage: () => void;
  } = $props();

  function formatResourceType(rt: string | null): string {
    if (!rt) return '—';
    const map: Record<string, string> = {
      user: 'User',
      repo: 'Repository',
      org: 'Organization',
    };
    return map[rt] || rt;
  }
</script>

<div class="table-wrap">
  <table class="audit-table">
    <thead>
      <tr>
        <th>{t('admin.audit.fields.time')}</th>
        <th>{t('admin.audit.fields.user')}</th>
        <th>{t('admin.audit.fields.action')}</th>
        <th>{t('admin.audit.fields.resource')}</th>
        <th>{t('admin.audit.fields.ip')}</th>
        <th></th>
      </tr>
    </thead>
    <tbody>
      {#each logs as log}
        <tr>
          <td class="time-cell" title={formatDateTime(log.created_at)}>
            {formatDate(log.created_at)}
          </td>
          <td class="user-cell">
            {#if log.username}
              <span class="username-text">{log.username}</span>
            {:else if log.user_id}
              <span class="user-id">#{log.user_id}</span>
            {:else}
              <span class="anonymous">—</span>
            {/if}
          </td>
          <td>
            <span class="action-badge" data-action={log.action}>
              {log.action}
            </span>
          </td>
          <td class="resource-cell">
            {#if log.resource_name}
              <span class="resource-link">{formatResourceType(log.resource_type)}: {log.resource_name}</span>
            {:else}
              <span class="text-muted">—</span>
            {/if}
          </td>
          <td class="ip-cell">
            {log.ip_address || '—'}
          </td>
          <td class="actions">
            <button class="btn-sm" onclick={() => onOpenDetail(log)}>
              {t('admin.audit.fields.details')}
            </button>
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

{#if totalPages > 1}
  <div class="pagination">
    <button onclick={onPrevPage} disabled={page <= 0}>← Prev</button>
    <span>Page {page + 1} of {totalPages}</span>
    <button onclick={onNextPage} disabled={page >= totalPages - 1}>Next →</button>
  </div>
{/if}

<style>
  .table-wrap { overflow-x: auto; }
  .audit-table { width: 100%; border-collapse: collapse; font-size: 0.875rem; }
  .audit-table th {
    text-align: left;
    padding: 0.6rem 0.75rem;
    border-bottom: 2px solid var(--border);
    color: var(--text-secondary);
    font-weight: 600;
    white-space: nowrap;
  }
  .audit-table td {
    padding: 0.6rem 0.75rem;
    border-bottom: 1px solid var(--border);
    color: var(--text-primary);
  }
  .audit-table tr:hover td { background: var(--bg-hover); }

  .time-cell {
    white-space: nowrap;
    color: var(--text-secondary);
    font-size: 0.8rem;
    font-family: var(--font-mono, monospace);
  }
  .user-cell .username-text { font-weight: 500; }
  .user-cell .user-id { color: var(--text-secondary); }
  .user-cell .anonymous { color: var(--text-secondary); }
  .resource-cell { max-width: 240px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .resource-link { color: var(--accent); }
  .text-muted { color: var(--text-secondary); }
  .ip-cell {
    font-family: var(--font-mono, monospace);
    font-size: 0.8rem;
    color: var(--text-secondary);
    white-space: nowrap;
  }
  .actions { display: flex; gap: 0.5rem; }

  /* Action badge */
  .action-badge {
    display: inline-block;
    padding: 0.15rem 0.5rem;
    border-radius: 8px;
    font-size: 0.78rem;
    font-weight: 500;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    white-space: nowrap;
    font-family: var(--font-mono, monospace);
  }
  .action-badge[data-action^="user."] { border-color: #58a6ff; color: #58a6ff; background: rgba(88, 166, 255, 0.1); }
  .action-badge[data-action^="repo."] { border-color: #3fb950; color: #3fb950; background: rgba(63, 185, 80, 0.1); }
  .action-badge[data-action^="org."] { border-color: #d2a8ff; color: #d2a8ff; background: rgba(210, 168, 255, 0.1); }
  .action-badge[data-action^="admin."] { border-color: #f85149; color: #f85149; background: rgba(248, 81, 73, 0.1); }

  /* Pagination */
  .pagination { display: flex; align-items: center; gap: 1rem; margin-top: 1rem; }
  .pagination button {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    color: var(--text-primary);
    border-radius: 6px;
    padding: 0.4rem 0.8rem;
    cursor: pointer;
  }
  .pagination button:disabled { opacity: 0.5; cursor: not-allowed; }
  .pagination span { color: var(--text-secondary); font-size: 0.9rem; }

  .btn-sm {
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    color: var(--text-primary);
    border-radius: 4px;
    padding: 0.25rem 0.6rem;
    font-size: 0.8rem;
    cursor: pointer;
  }
  .btn-sm:hover { background: var(--bg-hover); }
</style>
