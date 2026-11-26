<script lang="ts">
  // Releases page — orchestration layer: loads the paginated release list and
  // each release's assets; cards handle their own delete/download actions.
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import { page } from '$app/state';
  import { releases, type ReleaseAsset } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import { toErrorMessage } from '$lib/utils/error';
  import { createLatest } from '$lib/utils/latest';
  import type { Release } from '$lib/types/entities';
  import ReleaseList from '$lib/components/releases/ReleaseList.svelte';

  const t = createT();
  const latest = createLatest();

  let owner = $derived(page.params.owner!);
  let repo = $derived(page.params.repo!);

  let releaseList = $state<Release[]>([]);
  let loading = $state(true);
  let error = $state('');
  let currentPage = $state(1);
  let totalPages = $state(1);
  let releaseAssets = $state<Record<number, ReleaseAsset[]>>({});

  const newReleaseHref = $derived(`/${owner}/${repo}/releases/new`);

  $effect(() => {
    void loadReleases();
  });

  async function loadReleases() {
    const reqOwner = owner;
    const reqRepo = repo;
    const reqPage = currentPage;
    const isCurrent = latest.begin();
    loading = true;
    error = '';
    try {
      const res = await releases.list(reqOwner, reqRepo, reqPage, 20);
      if (!isCurrent()) return;
      releaseList = res.data;
      totalPages = res.pagination?.total_pages ?? 1;
      await loadReleaseAssets(reqOwner, reqRepo, releaseList, isCurrent);
    } catch (e: unknown) {
      if (!isCurrent()) return;
      error = toErrorMessage(e, t('errors.load_failed', 'Load failed'));
    } finally {
      if (isCurrent()) {
        loading = false;
      }
    }
  }

  async function loadReleaseAssets(
    reqOwner: string,
    reqRepo: string,
    items: Release[],
    isCurrent: () => boolean,
  ) {
    const entries = await Promise.all(
      items.map(async (release) => {
        try {
          const assets = await releases.listAssets(reqOwner, reqRepo, release.id);
          return [release.id, assets] as const;
        } catch {
          return [release.id, []] as const;
        }
      })
    );

    if (!isCurrent()) return;
    releaseAssets = Object.fromEntries(entries);
  }

  function goToPage(nextPage: number) {
    currentPage = nextPage;
    loadReleases();
  }
</script>

<svelte:head>
  <title>{t('repo.tabs.releases')} · {owner}/{repo} · IronForge</title>
</svelte:head>

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

<style>

  @media (max-width: 600px) {
    .page-header {
      flex-direction: column;
      align-items: flex-start;
      gap: 12px;
    }
  }
</style>
