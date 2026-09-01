<script lang="ts">
  import { isAuthReady, isLoggedIn, isAdmin } from '$lib/stores/auth.svelte';
  import { goto } from '$app/navigation';
  import { createT } from '$lib/i18n';
  import { admin, type AdminUser } from '$lib/api/client.svelte';
  import AdminUserTable from '$lib/components/admin/AdminUserTable.svelte';
  import UserEditModal from '$lib/components/admin/UserEditModal.svelte';
  import UserDeleteModal from '$lib/components/admin/UserDeleteModal.svelte';

  const t = createT();

  let users = $state<AdminUser[]>([]);
  let page = $state(1);
  let perPage = $state(20);
  let totalPages = $state(1);
  let total = $state(0);
  let loading = $state(true);
  let error = $state('');
  let editTarget = $state<AdminUser | null>(null);
  let deleteTarget = $state<AdminUser | null>(null);

  $effect(() => {
    if (!isAuthReady()) return;
    if (!isLoggedIn()) { goto('/login'); return; }
    if (!isAdmin()) { goto('/dashboard'); return; }
    loadUsers();
  });

  async function loadUsers() {
    loading = true;
    error = '';
    try {
      const result = await admin.listUsers(page, perPage);
      users = result.data;
      total = result.pagination?.total ?? 0;
      totalPages = result.pagination?.total_pages ?? 1;
    } catch (e: any) {
      error = e.message || t('errors.load_failed');
    } finally {
      loading = false;
    }
  }

  function handlePageChange(next: number) {
    page = next;
    loadUsers();
  }
</script>

<div class="container">
  <div class="header">
    <a href="/admin" class="back">← {t('admin.back')}</a>
    <h1>{t('admin.users.title')}</h1>
    <p class="meta">{total} {t('admin.users.total')}</p>
  </div>

  {#if error}
    <div class="error">{error}</div>
  {/if}

  {#if loading}
    <p class="loading">{t('common.loading')}</p>
  {:else}
    <AdminUserTable
      users={users}
      page={page}
      totalPages={totalPages}
      onPageChange={handlePageChange}
      onEdit={(u) => (editTarget = u)}
      onDelete={(u) => (deleteTarget = u)}
      onRefresh={loadUsers}
    />
  {/if}
</div>

{#if editTarget}
  <UserEditModal
    user={editTarget}
    onClose={() => (editTarget = null)}
    onSaved={loadUsers}
  />
{/if}

{#if deleteTarget}
  <UserDeleteModal
    user={deleteTarget}
    onClose={() => (deleteTarget = null)}
    onDeleted={loadUsers}
  />
{/if}

<style>
  .header { margin-bottom: 1.5rem; }
  .back { color: var(--text-secondary); text-decoration: none; font-size: 0.9rem; }
  .back:hover { color: var(--accent); text-decoration: none; }
  h1 { margin: 0.5rem 0 0; }
  .meta { color: var(--text-secondary); margin: 0; }
  .error { color: #f85149; background: rgba(248, 81, 73, 0.1); padding: 0.5rem 0.75rem; border-radius: 6px; margin-bottom: 1rem; }
  .loading { color: var(--text-secondary); }
</style>
