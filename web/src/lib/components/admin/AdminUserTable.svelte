<script lang="ts">
  import { admin, type AdminUser } from '$lib/api/client.svelte';
  import { getUser } from '$lib/stores/auth.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import { createT, formatDate } from '$lib/i18n';

  const t = createT();

  let {
    users,
    page,
    totalPages,
    onPageChange,
    onEdit,
    onDelete,
    onRefresh,
  }: {
    users: AdminUser[];
    page: number;
    totalPages: number;
    onPageChange: (page: number) => void;
    onEdit: (user: AdminUser) => void;
    onDelete: (user: AdminUser) => void;
    onRefresh: () => void | Promise<void>;
  } = $props();

  let unlockingUserId = $state<number | null>(null);

  function isLocked(user: AdminUser) {
    return !!user.locked_until && new Date(user.locked_until).getTime() > Date.now();
  }

  async function handleUnlock(user: AdminUser) {
    try {
      unlockingUserId = user.id;
      await admin.unlockUser(user.id);
      await onRefresh();
    } catch (e: any) {
      toast.error(toErrorMessage(e, t('errors.load_failed')));
    } finally {
      unlockingUserId = null;
    }
  }
</script>

<div class="table-wrap">
  <table class="users-table">
    <thead>
      <tr>
        <th>Username</th>
        <th>Email</th>
        <th>Admin</th>
        <th>Active</th>
        <th>Provider</th>
        <th>Login</th>
        <th>Created</th>
        <th></th>
      </tr>
    </thead>
    <tbody>
      {#each users as u (u.id)}
        <tr>
          <td class="username">{u.username}</td>
          <td class="email">{u.email}</td>
          <td>
            <span class="badge" class:admin={u.is_admin}>
              {u.is_admin ? '✓' : '—'}
            </span>
          </td>
          <td>
            <span class="badge" class:active={u.is_active} class:inactive={!u.is_active}>
              {u.is_active ? '✓' : '✗'}
            </span>
          </td>
          <td><span class="badge">{u.auth_provider}</span></td>
          <td class="login-state">
            {#if isLocked(u)}
              <span class="badge locked" title={`Locked until ${formatDate(u.locked_until || '')}`}
                >Locked</span
              >
            {:else if u.login_attempts > 0}
              <span class="badge warning">{u.login_attempts} failed</span>
            {:else}
              <span
                class="badge active"
                title={u.last_login_at
                  ? `Last login ${formatDate(u.last_login_at)}`
                  : 'No completed login recorded'}>OK</span
              >
            {/if}
          </td>
          <td class="date">{formatDate(u.created_at)}</td>
          <td class="actions">
            {#if isLocked(u) || u.login_attempts > 0}
              <button
                class="btn-sm"
                disabled={unlockingUserId === u.id}
                onclick={() => handleUnlock(u)}
              >
                {unlockingUserId === u.id ? 'Unlocking...' : 'Unlock'}
              </button>
            {/if}
            <button class="btn-sm" onclick={() => onEdit(u)}>{t('common.edit')}</button>
            {#if u.id !== getUser()?.id}
              <button class="btn-danger" onclick={() => onDelete(u)}>{t('common.delete')}</button>
            {/if}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

{#if totalPages > 1}
  <div class="pagination">
    <button onclick={() => onPageChange(page - 1)} disabled={page <= 1}>← Prev</button>
    <span>Page {page} of {totalPages}</span>
    <button onclick={() => onPageChange(page + 1)} disabled={page >= totalPages}>Next →</button>
  </div>
{/if}

<style>
  .table-wrap {
    overflow-x: auto;
  }

  .users-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.9rem;
  }

  .users-table th {
    text-align: left;
    padding: 0.6rem 0.75rem;
    border-bottom: 2px solid var(--border);
    color: var(--text-secondary);
    font-weight: 600;
  }

  .users-table td {
    padding: 0.6rem 0.75rem;
    border-bottom: 1px solid var(--border);
    color: var(--text-primary);
  }

  .users-table tr:hover td {
    background: var(--bg-hover);
  }

  .username {
    font-weight: 500;
  }

  .email {
    color: var(--text-secondary);
    font-size: 0.85rem;
  }

  .date {
    color: var(--text-secondary);
    font-size: 0.85rem;
    white-space: nowrap;
  }

  .actions {
    display: flex;
    gap: 0.5rem;
  }

  .login-state {
    white-space: nowrap;
  }

  .badge {
    display: inline-block;
    padding: 0.1rem 0.4rem;
    border-radius: 8px;
    font-size: 0.8rem;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
  }

  .badge.admin {
    color: var(--accent);
    border-color: var(--accent);
  }

  .badge.active {
    color: var(--green);
    border-color: var(--green);
  }

  .badge.inactive {
    color: #f85149;
    border-color: #f85149;
  }

  .badge.locked {
    color: #f85149;
    border-color: #f85149;
  }

  .badge.warning {
    color: var(--orange);
    border-color: var(--orange);
  }

  .btn-sm {
    padding: 0.25rem 0.6rem;
    font-size: 0.8rem;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 6px;
    cursor: pointer;
    color: var(--text-primary);
  }

  .btn-danger {
    padding: 0.25rem 0.6rem;
    font-size: 0.8rem;
    background: rgba(248, 81, 73, 0.1);
    border: 1px solid rgba(248, 81, 73, 0.4);
    border-radius: 6px;
    cursor: pointer;
    color: #f85149;
  }

  .pagination {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-top: 1rem;
  }

  .pagination button {
    padding: 0.4rem 0.9rem;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 6px;
    cursor: pointer;
    color: var(--text-primary);
  }

  .pagination button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
</style>
