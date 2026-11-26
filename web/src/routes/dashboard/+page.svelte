<script lang="ts">
  import { toErrorMessage } from '$lib/utils/error';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import Pagination from '$lib/components/Pagination.svelte';
  import { isLoggedIn, getUser } from '$lib/stores/auth.svelte';
  import { repos } from '$lib/api/client.svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { createT } from '$lib/i18n';
  import { createLatest } from '$lib/utils/latest';
  import { buildQueryHref, readPageParam } from '$lib/utils/urlQuery';
  import CreateRepoForm from '$lib/components/dashboard/CreateRepoForm.svelte';
  import RepoList from '$lib/components/dashboard/RepoList.svelte';

  const t = createT();
  const PER_PAGE = 20;

  type RepoListItem = { id: number; name: string; description: string | null; is_private: boolean; created_at: string };

  let owner = $derived(getUser()?.username || '');
  let currentPage = $derived(readPageParam(page.url));
  let repoList = $state<RepoListItem[]>([]);
  let total = $state(0);
  let loading = $state(true);
  let error = $state('');
  let showCreate = $state(false);

  const latest = createLatest();

  $effect(() => {
    if (page.url.searchParams.get('new') === '1') {
      showCreate = true;
    }
  });

  $effect(() => {
    if (!isLoggedIn()) {
      goto('/login');
      return;
    }
    void loadRepos();
  });

  async function loadRepos() {
    const reqOwner = owner;
    const reqPage = currentPage;
    if (!reqOwner) {
      loading = false;
      return;
    }
    const isCurrent = latest.begin();
    try {
      loading = true;
      error = '';
      const result = await repos.list(reqOwner, reqPage, PER_PAGE);
      if (!isCurrent()) return;
      repoList = result.data;
      total = result.pagination?.total ?? result.data.length;
      if (reqPage > 1 && result.data.length === 0 && total > 0) {
        void goto(buildQueryHref(page.url, { page: null, new: null }), {
          replaceState: true,
          keepFocus: true,
          noScroll: true,
        });
      }
    } catch (e) {
      if (!isCurrent()) return;
      error = toErrorMessage(e, t('common.unknown_error'));
    } finally {
      if (isCurrent()) {
        loading = false;
      }
    }
  }

  function changePage(nextPage: number) {
    void goto(buildQueryHref(page.url, { page: nextPage === 1 ? null : nextPage, new: null }), {
      keepFocus: true,
    });
  }

  async function handleCreated(repoName: string) {
    showCreate = false;
    await goto(`/${owner}/${repoName}`);
  }
</script>

<svelte:head>
  <title>{t('dashboard.title')} · IronForge</title>
</svelte:head>

<div class="page-container">
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
    <Pagination {total} page={currentPage} perPage={PER_PAGE} onPageChange={changePage} />
  {/if}
</div>

<style>
  .dashboard-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 24px;
  }

</style>
