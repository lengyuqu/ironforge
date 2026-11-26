<script lang="ts">
  import { toErrorMessage } from '$lib/utils/error';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import Pagination from '$lib/components/Pagination.svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { pulls } from '$lib/api/client.svelte';
  import type { PullRequest } from '$lib/types/entities';
  import PullFilterTabs from '$lib/components/pulls/PullFilterTabs.svelte';
  import PullList from '$lib/components/pulls/PullList.svelte';
  import PullCreateForm from '$lib/components/pulls/PullCreateForm.svelte';
  import { createT } from '$lib/i18n';
  import { createLatest } from '$lib/utils/latest';
  import { buildQueryHref, readEnumParam, readPageParam } from '$lib/utils/urlQuery';

  const t = createT();
  const PER_PAGE = 20;
  const PULL_STATES = ['open', 'closed', 'merged'] as const;

  let owner = $derived(page.params.owner!);
  let repo = $derived(page.params.repo!);

  let prList = $state<PullRequest[]>([]);
  let loading = $state(true);
  let error = $state('');
  let filterState = $derived(readEnumParam(page.url, 'state', 'open', PULL_STATES));
  let currentPage = $derived(readPageParam(page.url));
  let total = $state(0);
  let showCreate = $state(false);

  const latest = createLatest();

  $effect(() => {
    void loadPRs();
  });

  async function loadPRs() {
    const reqOwner = owner;
    const reqRepo = repo;
    const reqState = filterState;
    const reqPage = currentPage;
    const isCurrent = latest.begin();
    try {
      loading = true;
      error = '';
      const res = await pulls.list(reqOwner, reqRepo, reqState, reqPage, PER_PAGE);
      if (!isCurrent()) return;
      prList = res.data;
      total = res.pagination?.total ?? res.data.length;
      if (reqPage > 1 && res.data.length === 0 && total > 0) {
        void goto(buildQueryHref(page.url, { page: null }), {
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

  function changeFilter(next: string) {
    void goto(buildQueryHref(page.url, { state: next === 'open' ? null : next, page: null }), {
      keepFocus: true,
      noScroll: true,
    });
  }

  function changePage(nextPage: number) {
    void goto(buildQueryHref(page.url, { page: nextPage === 1 ? null : nextPage }), {
      keepFocus: true,
    });
  }

  async function handleCreated() {
    showCreate = false;
    await loadPRs();
  }
</script>

<svelte:head>
  <title>{t('repo.tabs.pulls')} · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="gh-toolbar pulls-toolbar">
  <PullFilterTabs
    filter={filterState}
    onFilterChange={changeFilter}
  />
  <button class="btn-primary" onclick={() => (showCreate = !showCreate)}>
    {t('pulls.new')}
  </button>
</div>

{#if showCreate}
  <PullCreateForm {owner} {repo} onCreated={handleCreated} onCancel={() => (showCreate = false)} />
{/if}

{#if error}
  <ErrorBanner message={error} />
{/if}

<PullList {owner} {repo} pullRequests={prList} {loading} filter={filterState} />

<Pagination {total} page={currentPage} perPage={PER_PAGE} onPageChange={changePage} />

<style>
  .pulls-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }

</style>
