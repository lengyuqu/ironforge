<script lang="ts">
  import { page } from '$app/stores';
  import { collaborators } from '$lib/api/client.svelte';
  import type { RepoCollaborator } from '$lib/types/entities';
  import CollaboratorAddForm from '$lib/components/settings/CollaboratorAddForm.svelte';
  import CollaboratorTable from '$lib/components/settings/CollaboratorTable.svelte';
  import { createT } from '$lib/i18n';

  const t = createT();
  const owner = $derived($page.params.owner!);
  const repo = $derived($page.params.repo!);

  let collaboratorList = $state<RepoCollaborator[]>([]);
  let loading = $state(true);
  let error = $state('');

  $effect(() => {
    loadCollaborators();
  });

  async function loadCollaborators() {
    try {
      loading = true;
      error = '';
      collaboratorList = await collaborators.list(owner, repo);
    } catch (err: any) {
      error = err.message || t('settings.collaborators.load_failed');
    } finally {
      loading = false;
    }
  }
</script>

<div class="collaborators-page">
  <div class="page-header">
    <div>
      <h1>{t('settings.collaborators.title')}</h1>
      <p>{t('settings.collaborators.desc')}</p>
    </div>
  </div>

  {#if error}
    <div class="error-box">{error}</div>
  {/if}

  <CollaboratorAddForm {owner} {repo} onAdded={loadCollaborators} />

  <CollaboratorTable
    {owner}
    {repo}
    collaborators={collaboratorList}
    {loading}
    onRefresh={loadCollaborators}
  />
</div>

<style>
  .collaborators-page {
    max-width: 900px;
  }

  .page-header {
    margin-bottom: 2rem;
  }

  h1 {
    font-size: 1.75rem;
    margin: 0 0 0.5rem;
    color: var(--text-primary);
  }

  p {
    margin: 0;
    color: var(--text-secondary);
    font-size: 0.95rem;
  }

  .error-box {
    padding: 0.75rem 1rem;
    border-radius: 6px;
    margin-bottom: 1rem;
    font-size: 0.9rem;
    background: rgba(220, 53, 69, 0.1);
    color: var(--red, #dc3545);
    border: 1px solid rgba(220, 53, 69, 0.3);
  }
</style>
