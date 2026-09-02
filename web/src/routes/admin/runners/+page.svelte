<script lang="ts">
  // Admin runners page — orchestration layer: auth guard, list loading
  // and pagination. Registration (with token banner) and delete
  // confirmation are handled by the self-contained components.
  import { goto } from '$app/navigation';
  import { createT } from '$lib/i18n';
  import { runners, type RunnerListItem } from '$lib/api/client.svelte';
  import { isAdmin, isAuthReady, isLoggedIn } from '$lib/stores/auth.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import RunnerRegisterForm from '$lib/components/admin/RunnerRegisterForm.svelte';
  import RunnerTable from '$lib/components/admin/RunnerTable.svelte';
  import RunnerDeleteModal from '$lib/components/admin/RunnerDeleteModal.svelte';

  const t = createT();

  let runnerList = $state<RunnerListItem[]>([]);
  let page = $state(1);
  let perPage = $state(20);
  let total = $state(0);
  let totalPages = $state(1);
  let loading = $state(true);
  let error = $state('');
  let deleteTarget = $state<RunnerListItem | null>(null);

  $effect(() => {
    if (!isAuthReady()) return;
    if (!isLoggedIn()) {
      goto('/login');
      return;
    }
    if (!isAdmin()) {
      goto('/dashboard');
      return;
    }
    loadRunners();
  });

  async function loadRunners() {
    loading = true;
    error = '';
    try {
      const result = await runners.list(page, perPage);
      runnerList = result.data;
      total = result.pagination?.total ?? runnerList.length;
      totalPages = result.pagination?.total_pages ?? 1;
    } catch (e: unknown) {
      error = toErrorMessage(e, t('errors.load_failed', 'Load failed'));
    } finally {
      loading = false;
    }
  }

  function prevPage() {
    if (page > 1) {
      page -= 1;
      loadRunners();
    }
  }

  function nextPage() {
    if (page < totalPages) {
      page += 1;
      loadRunners();
    }
  }
</script>

<svelte:head>
  <title>{t('admin.runners.title')} · IronForge</title>
</svelte:head>

<div class="container">
  <div class="header">
    <a href="/admin" class="back">← {t('admin.back')}</a>
    <h1>{t('admin.runners.title')}</h1>
    <p class="meta">{total} {t('admin.runners.total')}</p>
  </div>

  {#if error}
    <div class="error">{error}</div>
  {/if}

  <RunnerRegisterForm onRegistered={loadRunners} />

  {#if loading}
    <p class="loading">{t('common.loading')}</p>
  {:else if runnerList.length === 0}
    <p class="empty">{t('admin.runners.empty')}</p>
  {:else}
    <RunnerTable items={runnerList} onDelete={(runner) => (deleteTarget = runner)} />

    {#if totalPages > 1}
      <div class="pagination">
        <button onclick={prevPage} disabled={page <= 1}>{t('common.previous')}</button>
        <span>Page {page} of {totalPages}</span>
        <button onclick={nextPage} disabled={page >= totalPages}>{t('common.next')}</button>
      </div>
    {/if}
  {/if}
</div>

{#if deleteTarget}
  <RunnerDeleteModal
    runner={deleteTarget}
    onClose={() => (deleteTarget = null)}
    onDeleted={loadRunners}
  />
{/if}

<style>
  .header {
    margin-bottom: 1.5rem;
  }

  .back {
    color: var(--text-secondary);
    text-decoration: none;
    font-size: 0.9rem;
  }

  .back:hover {
    color: var(--accent);
    text-decoration: none;
  }

  h1 {
    margin: 0.5rem 0 0;
  }

  .meta,
  .loading,
  .empty {
    color: var(--text-secondary);
  }

  .empty {
    font-style: italic;
  }

  .error {
    color: #f85149;
    background: rgba(248, 81, 73, 0.1);
    padding: 0.5rem 0.75rem;
    border-radius: 6px;
    margin-bottom: 1rem;
  }

  .pagination {
    display: flex;
    align-items: center;
    gap: 1rem;
    margin-top: 1rem;
  }

  .pagination button {
    background: var(--bg-primary);
    border: 1px solid var(--border);
    color: var(--text-primary);
    border-radius: 6px;
    padding: 0.45rem 0.8rem;
    cursor: pointer;
  }

  .pagination button:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
</style>
