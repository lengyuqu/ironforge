<script lang="ts">
  import { toErrorMessage } from '$lib/utils/error';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import { isAuthReady, isLoggedIn, isAdmin } from '$lib/stores/auth.svelte';
  import { goto } from '$app/navigation';
  import { createT, formatDate } from '$lib/i18n';
  import Modal from '$lib/components/Modal.svelte';
  import { admin, type AdminOrg } from '$lib/api/client.svelte';

  const t = createT();

  let orgs = $state<AdminOrg[]>([]);
  let page = $state(1);
  let perPage = $state(20);
  let totalPages = $state(1);
  let total = $state(0);
  let loading = $state(true);
  let error = $state('');
  let deleteTarget = $state<AdminOrg | null>(null);
  let showDeleteConfirm = $state(false);
  let deleting = $state(false);

  $effect(() => {
    if (!isAuthReady()) return;
    if (!isLoggedIn()) { goto('/login'); return; }
    if (!isAdmin()) { goto('/dashboard'); return; }
    loadOrgs();
  });

  async function loadOrgs() {
    loading = true;
    error = '';
    try {
      const result = await admin.listOrgs(page, perPage);
      orgs = result.data;
      total = result.pagination?.total ?? 0;
      totalPages = result.pagination?.total_pages ?? 1;
    } catch (e) {
      error = toErrorMessage(e, t('errors.load_failed'));
    } finally {
      loading = false;
    }
  }

  function confirmDelete(org: AdminOrg) {
    deleteTarget = org;
    showDeleteConfirm = true;
  }

  async function handleDelete() {
    if (!deleteTarget) return;
    deleting = true;
    error = '';
    try {
      await admin.deleteOrg(deleteTarget.name);
      deleteTarget = null;
      showDeleteConfirm = false;
      await loadOrgs();
    } catch (e) {
      error = toErrorMessage(e, t('common.unknown_error'));
    } finally {
      deleting = false;
    }
  }

  function prevPage() {
    if (page > 1) { page--; loadOrgs(); }
  }

  function nextPage() {
    if (page < totalPages) { page++; loadOrgs(); }
  }

</script>

<div class="container">
  <div class="header">
    <a href="/admin" class="back">← {t('admin.back')}</a>
    <h1>{t('admin.orgs.title')}</h1>
    <p class="meta">{total} {t('admin.orgs.total')}</p>
  </div>

  {#if error}
    <ErrorBanner message={error} />
  {/if}

  {#if loading}
    <LoadingState />
  {:else if orgs.length === 0}
    <EmptyState title={t('orgs.no_repos')} />
  {:else}
    <div class="table-wrap">
      <table class="orgs-table">
        <thead>
          <tr>
            <th>{t('admin.orgs.col.name')}</th>
            <th>{t('admin.users.display_name')}</th>
            <th>{t('admin.orgs.col.visibility')}</th>
            <th>{t('admin.orgs.col.owner_id')}</th>
            <th>{t('admin.orgs.col.created')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each orgs as org}
            <tr>
              <td class="name">
                <a href={`/orgs/${org.name}`}>{org.name}</a>
              </td>
              <td class="display-name">{org.display_name || '—'}</td>
              <td>
                <span class="badge" class:private={org.visibility === 'private'}>
                  {org.visibility}
                </span>
              </td>
              <td class="owner">#{org.owner_id}</td>
              <td class="date">{formatDate(org.created_at)}</td>
              <td class="actions">
                <button class="btn-danger" onclick={() => confirmDelete(org)}>{t('common.delete')}</button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    {#if totalPages > 1}
      <div class="pagination">
        <button onclick={prevPage} disabled={page <= 1}>← {t('common.previous')}</button>
        <span>{t('common.page_of', { page, total: totalPages })}</span>
        <button onclick={nextPage} disabled={page >= totalPages}>{t('common.next')} →</button>
      </div>
    {/if}
  {/if}
</div>

<!-- Delete confirm -->
  {#if showDeleteConfirm && deleteTarget}
    <Modal
      open={true}
      title={t('admin.orgs.delete_confirm')}
      submitLabel={deleting ? t('common.loading') : t('common.delete')}
      submitVariant="danger"
      busy={deleting}
      onClose={() => (showDeleteConfirm = false)}
      onSubmit={handleDelete}
    >
      <p class="modal-text">{t('admin.orgs.delete_warning', { name: deleteTarget.name })}</p>
      {#if error}
        <ErrorBanner message={error} />
      {/if}
    </Modal>
  {/if}

<style>
  .header { margin-bottom: 1.5rem; }
  .back { color: var(--text-secondary); text-decoration: none; font-size: 0.9rem; }
  .back:hover { color: var(--accent); text-decoration: none; }
  h1 { margin: 0.5rem 0 0; }
  .meta { color: var(--text-secondary); margin: 0; }

  .table-wrap { overflow-x: auto; }
  .orgs-table { width: 100%; border-collapse: collapse; font-size: 0.9rem; }
  .orgs-table th { text-align: left; padding: 0.6rem 0.75rem; border-bottom: 2px solid var(--border); color: var(--text-secondary); font-weight: 600; }
  .orgs-table td { padding: 0.6rem 0.75rem; border-bottom: 1px solid var(--border); color: var(--text-primary); }
  .orgs-table tr:hover td { background: var(--bg-hover); }
  .name a { color: var(--accent); font-weight: 500; text-decoration: none; }
  .name a:hover { text-decoration: underline; }
  .display-name { color: var(--text-secondary); }
  .owner { color: var(--text-secondary); }
  .date { color: var(--text-secondary); white-space: nowrap; }
  .actions { text-align: right; }

  .badge { display: inline-block; padding: 0.1rem 0.4rem; border-radius: 8px; font-size: 0.8rem; background: color-mix(in srgb, var(--green) 15%, transparent); color: var(--green); border: 1px solid var(--green); }
  .badge.private { background: color-mix(in srgb, var(--red) 15%, transparent); color: var(--red); border-color: var(--red); }

  .pagination { display: flex; align-items: center; gap: 1rem; margin-top: 1rem; }
  .pagination button { background: var(--bg-secondary); border: 1px solid var(--border); color: var(--text-primary); border-radius: 6px; padding: 0.4rem 0.8rem; cursor: pointer; }
  .pagination button:disabled { opacity: 0.5; cursor: not-allowed; }
  .pagination span { color: var(--text-secondary); font-size: 0.9rem; }

  /* Modal */
  .modal-text { color: var(--text-secondary); margin: 0 0 1rem; }
</style>
