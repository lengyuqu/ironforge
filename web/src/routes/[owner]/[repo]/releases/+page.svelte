<script lang="ts">
  // Releases page — orchestration layer: loads the paginated release list and
  // each release's assets; cards handle their own delete/download actions.
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
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
    <ErrorBanner message={error} />
  {/if}

  {#if loading}
    <LoadingState />
  {:else if releaseList.length === 0}
    <EmptyState title={t('releases.no_releases')}>
      <a href={newReleaseHref} class="btn-primary">{t('releases.new')}</a>
    </EmptyState>
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

  @media (max-width: 600px) {
    .page-header {
      flex-direction: column;
      align-items: flex-start;
      gap: 12px;
    }
  }
</style>
