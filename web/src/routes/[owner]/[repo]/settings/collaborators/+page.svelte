<script lang="ts">
  import { toErrorMessage } from '$lib/utils/error';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import { page } from '$app/state';
  import { collaborators } from '$lib/api/client.svelte';
  import type { RepoCollaborator } from '$lib/types/entities';
  import CollaboratorAddForm from '$lib/components/settings/CollaboratorAddForm.svelte';
  import CollaboratorTable from '$lib/components/settings/CollaboratorTable.svelte';
  import { createT } from '$lib/i18n';
  import { createLatest } from '$lib/utils/latest';

  const t = createT();
  const latest = createLatest();
  const owner = $derived(page.params.owner!);
  const repo = $derived(page.params.repo!);

  let collaboratorList = $state<RepoCollaborator[]>([]);
  let loading = $state(true);
  let error = $state('');

  $effect(() => {
    void loadCollaborators();
  });

  async function loadCollaborators() {
    const reqOwner = owner;
    const reqRepo = repo;
    const isCurrent = latest.begin();
    try {
      loading = true;
      error = '';
      const list = await collaborators.list(reqOwner, reqRepo);
      if (!isCurrent()) return;
      collaboratorList = list;
    } catch (err) {
      if (!isCurrent()) return;
      error = toErrorMessage(err, t('settings.collaborators.load_failed'));
    } finally {
      if (isCurrent()) {
        loading = false;
      }
    }
  }
</script>

<svelte:head>
  <title>{t('settings.collaborators.title')} · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="collaborators-page">
  <div class="page-header">
    <div>
      <h1>{t('settings.collaborators.title')}</h1>
      <p>{t('settings.collaborators.desc')}</p>
    </div>
  </div>

  {#if error}
    <ErrorBanner message={error} />
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

  h1 {
    margin: 0 0 0.5rem;
    color: var(--text-primary);
  }

  p {
    margin: 0;
    color: var(--text-secondary);
    font-size: 0.95rem;
  }

</style>
