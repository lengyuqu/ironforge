<script lang="ts">
  import LoadingState from '$lib/components/LoadingState.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import { confirmDialog } from '$lib/components/confirm.svelte';
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
    } catch (err) {
      toast.error(toErrorMessage(err, t('settings.collaborators.update_failed')));
    } finally {
      busyId = null;
    }
  }

  async function removeCollaborator(collaborator: RepoCollaborator) {
    if (!(await confirmDialog({ variant: 'danger', message: t('settings.collaborators.remove_confirm', { userId: collaborator.user_id }) }))) return;

    try {
      busyId = collaborator.id;
      await collaboratorsApi.remove(owner, repo, collaborator.user_id);
      toast.success(t('settings.collaborators.removed'));
      await onRefresh();
    } catch (err) {
      toast.error(toErrorMessage(err, t('settings.collaborators.remove_failed')));
    } finally {
      busyId = null;
    }
  }
</script>

<section class="settings-section">
  <h2>{t('settings.collaborators.current')}</h2>

  {#if loading}
    <LoadingState />
  {:else if collaboratorList.length === 0}
    <EmptyState title={t('settings.collaborators.empty')} />
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
    margin: 0 0 1rem;
    color: var(--text-primary);
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
  }

  @media (max-width: 720px) {
    .actions {
      justify-content: flex-start;
    }
  }
</style>
