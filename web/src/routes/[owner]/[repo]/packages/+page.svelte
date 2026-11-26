<script lang="ts">
  import { toErrorMessage } from '$lib/utils/error';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import { page } from '$app/state';
  import { packages, type PackageSummaryResponse } from '$lib/api/client.svelte';
  import PackageList from '$lib/components/packages/PackageList.svelte';
  import { createT } from '$lib/i18n';
  import { createLatest } from '$lib/utils/latest';
  import { PACKAGE_FORMATS, packageFormatOptionLabel } from '$lib/packageFormats';

  const t = createT();
  const latest = createLatest();

  let owner = $derived(page.params.owner!);
  let repo = $derived(page.params.repo!);
  let formatFilter = $state<string>('');
  let searchQuery = $state<string>('');
  let packageList = $state<PackageSummaryResponse[]>([]);
  let loading = $state(true);
  let error = $state('');
  let currentPage = $state(1);
  let totalPages = $state(1);

  $effect(() => {
    void loadPackages();
  });

  async function loadPackages() {
    const reqOwner = owner!;
    const reqRepo = repo!;
    const reqFormat = formatFilter || undefined;
    const reqPage = currentPage;
    const reqSearch = searchQuery;
    const isCurrent = latest.begin();
    loading = true;
    error = '';
    try {
      const res = await packages.list(
        reqOwner,
        reqRepo,
        reqFormat,
        reqPage,
        20,
        reqSearch,
      );
      if (!isCurrent()) return;
      packageList = res.data;
      totalPages = res.pagination.total_pages;
    } catch (e) {
      if (!isCurrent()) return;
      error = toErrorMessage(e, t('common.unknown_error'));
    } finally {
      if (isCurrent()) {
        loading = false;
      }
    }
  }

  function handleFormatChange() {
    currentPage = 1;
    loadPackages();
  }

  function handleSearch() {
    currentPage = 1;
    loadPackages();
  }

  function handlePageChange(next: number) {
    currentPage = next;
    loadPackages();
  }
</script>

<svelte:head>
  <title>{t('repo.tabs.packages')} · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-header">
  <h1>{t('repo.tabs.packages')}</h1>
  <a href={`/${owner}/${repo}/packages/upload`} class="btn-primary">{t('packages.upload')}</a>
</div>

{#if error}
  <ErrorBanner message={error} />
{/if}

<div class="filters">
  <div class="filter-group">
    <label for="format-filter">{t('packages.format')}:</label>
    <select id="format-filter" bind:value={formatFilter} onchange={handleFormatChange}>
      <option value="">{t('common.all') || 'All'}</option>
      {#each PACKAGE_FORMATS as f (f)}
        <option value={f}>{packageFormatOptionLabel(f)}</option>
      {/each}
    </select>
  </div>

  <div class="search-group">
    <input
      type="text"
      placeholder={t('common.search') || 'Search...'}
      bind:value={searchQuery}
      onkeydown={(e) => e.key === 'Enter' && handleSearch()}
    />
    <button class="btn-secondary" onclick={handleSearch}>{t('common.search') || 'Search'}</button>
  </div>
</div>

{#if loading}
  <LoadingState />
{:else}
  <PackageList
    owner={owner!}
    repo={repo!}
    packages={packageList}
    currentPage={currentPage}
    totalPages={totalPages}
    onPageChange={handlePageChange}
  />
{/if}

<style>

  .filters {
    display: flex;
    gap: 16px;
    margin-bottom: 24px;
    flex-wrap: wrap;
  }

  .filter-group {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .filter-group label {
    font-size: 14px;
    color: var(--text-secondary);
  }

  .search-group {
    display: flex;
    gap: 8px;
    flex: 1;
    max-width: 400px;
  }

  .search-group input {
    flex: 1;
  }

</style>
