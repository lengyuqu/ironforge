<script lang="ts">
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import { isLoggedIn, getUser } from '$lib/stores/auth.svelte';
  import { repos } from '$lib/api/client.svelte';
  import { goto } from '$app/navigation';
  import { createT } from '$lib/i18n';
  import CreateRepoForm from '$lib/components/dashboard/CreateRepoForm.svelte';
  import RepoList from '$lib/components/dashboard/RepoList.svelte';

  const t = createT();

  type RepoListItem = { id: number; name: string; description: string | null; is_private: boolean; created_at: string };

  let owner = $derived(getUser()?.username || '');
  let repoList = $state<RepoListItem[]>([]);
  let loading = $state(true);
  let error = $state('');
  let showCreate = $state(false);

  $effect(() => {
    if (!isLoggedIn()) {
      goto('/login');
      return;
    }
    loadRepos();
  });

  async function loadRepos() {
    if (!owner) return;
    try {
      loading = true;
      const result = await repos.list(owner);
      repoList = result.data;
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  async function handleCreated(repoName: string) {
    showCreate = false;
    await goto(`/${owner}/${repoName}`);
  }
</script>

<svelte:head>
  <title>{t('dashboard.title')} · IronForge</title>
</svelte:head>

<div class="dashboard">
  <div class="dashboard-header">
    <h1>{t('dashboard.title')}</h1>
    <button class="btn-primary" onclick={() => showCreate = !showCreate}>
      + {t('dashboard.new_repo')}
    </button>
  </div>

  {#if showCreate}
    <CreateRepoForm owner={owner} onCreated={handleCreated} onCancel={() => showCreate = false} />
  {/if}

  {#if error}
    <ErrorBanner message={error} />
  {/if}

  {#if loading}
    <LoadingState />
  {:else if repoList.length === 0}
    <EmptyState title={t('dashboard.empty.no_repos')} description={t('dashboard.empty.get_started')} />
  {:else}
    <RepoList owner={owner} repos={repoList} />
  {/if}
</div>

<style>
  .dashboard-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 24px;
  }

  h1 { font-size: 24px; }

  .btn-primary {
    padding: 6px 16px;
    background: var(--green-dim);
    color: #fff;
    border: none;
    border-radius: var(--radius);
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }
  .btn-primary:hover { background: var(--green); }

</style>
