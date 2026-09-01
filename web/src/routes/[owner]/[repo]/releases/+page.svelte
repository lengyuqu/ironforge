<script lang="ts">
  // Releases page — orchestration layer: loads the paginated release list and
  // each release's assets; cards handle their own delete/download actions.
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import { releases, type ReleaseAsset } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import type { Release } from '$lib/types/entities';
  import ReleaseList from '$lib/components/releases/ReleaseList.svelte';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);

  let releaseList = $state<Release[]>([]);
  let loading = $state(true);
  let error = $state('');
  let currentPage = $state(1);
  let totalPages = $state(1);
  let releaseAssets = $state<Record<number, ReleaseAsset[]>>({});

  const newReleaseHref = $derived(`/${owner}/${repo}/releases/new`);

  $effect(() => {
    loadReleases();
  });

  async function loadReleases() {
    loading = true;
    error = '';
    try {
      const res = await releases.list(owner, repo, currentPage, 20);
      releaseList = res.data;
      totalPages = res.pagination?.total_pages ?? 1;
      await loadReleaseAssets(releaseList);
    } catch (e: unknown) {
      error = toErrorMessage(e, t('errors.load_failed', 'Load failed'));
    } finally {
      loading = false;
    }
  }

  async function loadReleaseAssets(items: Release[]) {
    const entries = await Promise.all(
      items.map(async (release) => {
        try {
          const assets = await releases.listAssets(owner, repo, release.id);
          return [release.id, assets] as const;
        } catch {
          return [release.id, []] as const;
        }
      })
    );

    releaseAssets = Object.fromEntries(entries);
  }

  function goToPage(nextPage: number) {
    currentPage = nextPage;
    loadReleases();
  }
</script>

<svelte:head>
  <title>Releases · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-container">
  <RepoHeader {owner} {repo} activeTab="releases" />

  <div class="page-header">
    <h1>{t('releases.title')}</h1>
    <a href={newReleaseHref} class="btn-primary">{t('releases.new')}</a>
  </div>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if loading}
    <p class="loading-text">{t('common.loading')}</p>
  {:else if releaseList.length === 0}
    <div class="empty">
      <p>{t('releases.no_releases')}</p>
      <a href={newReleaseHref} class="btn-primary">{t('releases.new')}</a>
    </div>
  {:else}
    <ReleaseList
      {owner}
      {repo}
      releases={releaseList}
      assets={releaseAssets}
      {currentPage}
      {totalPages}
      onReload={loadReleases}
      onPageChange={goToPage}
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

  .loading-text {
    color: var(--text-secondary);
    text-align: center;
    padding: 48px;
  }

  .empty {
    text-align: center;
    padding: 48px;
    color: var(--text-secondary);
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }

  .empty p {
    margin-bottom: 16px;
  }

  @media (max-width: 600px) {
    .page-header {
      flex-direction: column;
      align-items: flex-start;
      gap: 12px;
    }
  }
</style>
