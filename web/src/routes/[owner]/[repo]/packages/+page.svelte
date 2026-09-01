<script lang="ts">
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { packages, type PackageSummaryResponse } from '$lib/api/client.svelte';
  import PackageList from '$lib/components/packages/PackageList.svelte';
  import { createT } from '$lib/i18n';
  import { PACKAGE_FORMATS, packageFormatOptionLabel } from '$lib/packageFormats';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  let formatFilter = $state<string>('');
  let searchQuery = $state<string>('');
  let packageList = $state<PackageSummaryResponse[]>([]);
  let loading = $state(true);
  let error = $state('');
  let currentPage = $state(1);
  let totalPages = $state(1);

  $effect(() => {
    loadPackages();
  });

  async function loadPackages() {
    loading = true;
    error = '';
    try {
      const res = await packages.list(
        owner!,
        repo!,
        formatFilter || undefined,
        currentPage,
        20,
        searchQuery,
      );
      packageList = res.data;
      totalPages = res.pagination.total_pages;
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
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
  <title>Packages · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-container">
  <RepoHeader owner={owner!} repo={repo!} activeTab="packages" />

  <div class="page-header">
    <h1>{t('repo.tabs.packages')}</h1>
    <a href={`/${owner}/${repo}/packages/upload`} class="btn-primary">{t('packages.upload')}</a>
  </div>

  {#if error}
    <div class="error-banner">{error}</div>
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
    <p class="loading-text">{t('common.loading')}</p>
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
</div>

<style>
  .page-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 24px;
  }

  h1 {
    font-size: 24px;
    font-weight: 600;
  }

  .btn-primary {
    padding: 6px 16px;
    background: var(--orange);
    color: #fff;
    border: none;
    border-radius: var(--radius);
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    text-decoration: none;
  }

  .btn-primary:hover {
    background: #e09a1e;
    text-decoration: none;
  }

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

  .filter-group select {
    padding: 6px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-secondary);
    color: var(--text-primary);
    font-size: 14px;
  }

  .search-group {
    display: flex;
    gap: 8px;
    flex: 1;
    max-width: 400px;
  }

  .search-group input {
    flex: 1;
    padding: 6px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-secondary);
    color: var(--text-primary);
    font-size: 14px;
  }

  .btn-secondary {
    padding: 6px 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text-primary);
    font-size: 14px;
    cursor: pointer;
  }

  .btn-secondary:hover {
    background: var(--bg-tertiary, #e5e7eb);
  }

  .loading-text {
    color: var(--text-secondary);
    text-align: center;
    padding: 48px;
  }

  .error-banner {
    color: #f85149;
    background: rgba(248, 81, 73, 0.1);
    padding: 10px 12px;
    border-radius: var(--radius);
    margin-bottom: 16px;
  }
</style>
