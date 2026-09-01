<script lang="ts">
  import { isAuthReady, isLoggedIn, isAdmin } from '$lib/stores/auth.svelte';
  import { goto } from '$app/navigation';
  import { createT } from '$lib/i18n';
  import { admin, type AuditLogEntry } from '$lib/api/client.svelte';
  import AuditFilters from '$lib/components/admin/AuditFilters.svelte';
  import AuditLogTable from '$lib/components/admin/AuditLogTable.svelte';
  import AuditDetailModal from '$lib/components/admin/AuditDetailModal.svelte';

  const t = createT();

  let logs = $state<AuditLogEntry[]>([]);
  let page = $state(0);
  let perPage = $state(20);
  let total = $state(0);
  let totalPages = $state(1);
  let loading = $state(true);
  let error = $state('');

  // Filter state
  let actionFilter = $state('');
  let resourceFilter = $state('');

  // Detail modal
  let selectedLog = $state<AuditLogEntry | null>(null);

  $effect(() => {
    if (!isAuthReady()) return;
    if (!isLoggedIn()) { goto('/login'); return; }
    if (!isAdmin()) { goto('/dashboard'); return; }
    loadLogs();
  });

  async function loadLogs() {
    loading = true;
    error = '';
    try {
      const result = await admin.listAuditLogs({
        page: page + 1, // L-4: Backend now uses 1-based page numbering
        per_page: perPage,
        action: actionFilter || undefined,
        resource_type: resourceFilter || undefined,
      });
      logs = result.logs;
      total = result.total;
      totalPages = Math.max(1, Math.ceil(result.total / result.per_page));
    } catch (e: any) {
      error = e.message || t('errors.load_failed');
    } finally {
      loading = false;
    }
  }

  function applyFilter() {
    page = 0;
    loadLogs();
  }

  function clearFilters() {
    actionFilter = '';
    resourceFilter = '';
    page = 0;
    loadLogs();
  }

  function openDetail(log: AuditLogEntry) {
    selectedLog = log;
  }

  function closeDetail() {
    selectedLog = null;
  }

  function prevPage() {
    if (page > 0) { page--; loadLogs(); }
  }

  function nextPage() {
    if (page < totalPages - 1) { page++; loadLogs(); }
  }
</script>

<div class="container">
  <div class="header">
    <a href="/admin" class="back">← {t('admin.back')}</a>
    <h1>{t('admin.audit.title')}</h1>
    <p class="meta">{total} {t('admin.audit.total')}</p>
  </div>

  <AuditFilters
    bind:actionFilter
    bind:resourceFilter
    onApply={applyFilter}
    onClear={clearFilters}
  />

  {#if error}
    <div class="error">{error}</div>
  {/if}

  {#if loading}
    <p class="loading">{t('common.loading')}</p>
  {:else if logs.length === 0}
    <p class="empty">No audit records found.</p>
  {:else}
    <AuditLogTable
      {logs}
      {page}
      {totalPages}
      onOpenDetail={openDetail}
      onPrevPage={prevPage}
      onNextPage={nextPage}
    />
  {/if}
</div>

{#if selectedLog}
  <AuditDetailModal log={selectedLog} onClose={closeDetail} />
{/if}

<style>
  .header { margin-bottom: 1rem; }
  .back { color: var(--text-secondary); text-decoration: none; font-size: 0.9rem; }
  .back:hover { color: var(--accent); text-decoration: none; }
  h1 { margin: 0.5rem 0 0; }
  .meta { color: var(--text-secondary); margin: 0; }

  .error { color: #f85149; background: rgba(248, 81, 73, 0.1); padding: 0.5rem 0.75rem; border-radius: 6px; margin-bottom: 1rem; }
  .loading { color: var(--text-secondary); }
  .empty { color: var(--text-secondary); text-align: center; padding: 2rem 0; }
</style>
