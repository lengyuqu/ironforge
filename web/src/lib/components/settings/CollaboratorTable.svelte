<script lang="ts">
  import { collaborators as collaboratorsApi } from '$lib/api/client.svelte';
  import { toast } from '$lib/components/toast.svelte';
  import { toErrorMessage } from '$lib/utils/error';
  import type { RepoCollaborator } from '$lib/types/entities';
  import { createT } from '$lib/i18n';

  const t = createT();

  let {
    owner,
    repo,
    collaborators: collaboratorList,
    loading,
    onRefresh,
  }: {
    owner: string;
    repo: string;
    collaborators: RepoCollaborator[];
    loading: boolean;
    onRefresh: () => void | Promise<void>;
  } = $props();

  let busyId = $state<number | null>(null);

  const permissionOptions = [
    { value: 'read', label: t('orgs.permission.read') },
    { value: 'write', label: t('orgs.permission.write') },
    { value: 'admin', label: t('orgs.permission.admin') }
  ];

  async function savePermission(collaborator: RepoCollaborator) {
    try {
      busyId = collaborator.id;
      await collaboratorsApi.updatePermission(owner, repo, collaborator.id, collaborator.permission);
      toast.success(t('settings.collaborators.updated'));
      await onRefresh();
    } catch (err: any) {
      toast.error(toErrorMessage(err, t('settings.collaborators.update_failed')));
    } finally {
      busyId = null;
    }
  }

  async function removeCollaborator(collaborator: RepoCollaborator) {
    if (!confirm(t('settings.collaborators.remove_confirm', { userId: collaborator.user_id }))) return;

    try {
      busyId = collaborator.id;
      await collaboratorsApi.remove(owner, repo, collaborator.user_id);
      toast.success(t('settings.collaborators.removed'));
      await onRefresh();
    } catch (err: any) {
      toast.error(toErrorMessage(err, t('settings.collaborators.remove_failed')));
    } finally {
      busyId = null;
    }
  }
</script>

<section class="section">
  <h2>{t('settings.collaborators.current')}</h2>

  {#if loading}
    <div class="loading">{t('common.loading')}</div>
  {:else if collaboratorList.length === 0}
    <div class="empty-state">{t('settings.collaborators.empty')}</div>
  {:else}
    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            <th>{t('settings.collaborators.user')}</th>
            <th>{t('settings.collaborators.permission')}</th>
            <th>{t('settings.collaborators.created')}</th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {#each collaboratorList as collaborator (collaborator.id)}
            <tr>
              <td>
                <span class="user-id">#{collaborator.user_id}</span>
              </td>
              <td>
                <select
                  bind:value={collaborator.permission}
                  disabled={busyId === collaborator.id}
                >
                  {#each permissionOptions as option (option.value)}
                    <option value={option.value}>{option.label}</option>
                  {/each}
                </select>
              </td>
              <td>{new Date(collaborator.created_at).toLocaleDateString()}</td>
              <td class="actions">
                <button
                  class="btn btn-outline"
                  onclick={() => savePermission(collaborator)}
                  disabled={busyId === collaborator.id}
                >
                  {t('common.save')}
                </button>
                <button
                  class="btn btn-danger"
                  onclick={() => removeCollaborator(collaborator)}
                  disabled={busyId === collaborator.id}
                >
                  {t('common.delete')}
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>

<style>
  h2 {
    font-size: 1.1rem;
    margin: 0 0 1rem;
    color: var(--text-primary);
  }

  .section {
    margin-bottom: 2.5rem;
    padding-bottom: 2rem;
    border-bottom: 1px solid var(--border);
  }

  .loading,
  .empty-state {
    padding: 2rem;
    text-align: center;
    color: var(--text-secondary);
    background: var(--bg-secondary);
    border-radius: 6px;
  }

  .table-wrap {
    overflow-x: auto;
  }

  table {
    width: 100%;
    border-collapse: collapse;
  }

  th,
  td {
    padding: 0.75rem;
    border-bottom: 1px solid var(--border);
    text-align: left;
    color: var(--text-primary);
    vertical-align: middle;
  }

  th {
    color: var(--text-secondary);
    font-size: 0.85rem;
    font-weight: 600;
  }

  .user-id {
    font-family: monospace;
    font-weight: 600;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    white-space: nowrap;
  }

  select {
    min-height: 34px;
    padding: 0.4rem 0.6rem;
    background: var(--bg-primary);
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text-primary);
    font-size: 0.9rem;
  }

  .btn {
    padding: 0.55rem 0.9rem;
    border-radius: 6px;
    border: 1px solid var(--border);
    cursor: pointer;
    font-size: 0.9rem;
  }

  .btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .btn-outline {
    background: var(--bg-primary);
    color: var(--text-primary);
  }

  .btn-danger {
    background: var(--red, #dc3545);
    color: white;
    border-color: var(--red, #dc3545);
  }

  @media (max-width: 760px) {
    .actions {
      justify-content: flex-start;
    }
  }
</style>
