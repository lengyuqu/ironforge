<script lang="ts">
  import { toErrorMessage } from '$lib/utils/error';
  import ErrorBanner from '$lib/components/ErrorBanner.svelte';
  import LoadingState from '$lib/components/LoadingState.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import { page } from '$app/state';
  import {
    packages,
    type PackageDetailResponse,
    type PackageVersionResponse,
  } from '$lib/api/client.svelte';
  import PackageVersions from '$lib/components/packages/PackageVersions.svelte';
  import { createT } from '$lib/i18n';
  import { createLatest } from '$lib/utils/latest';
  import { packageFormatLabel } from '$lib/packageFormats';

  const t = createT();
  const latest = createLatest();

  let owner = $derived(page.params.owner!);
  let repo = $derived(page.params.repo!);
  let format = $derived(page.params.format!);
  let name = $derived(page.params.name!);

  let packageInfo = $state<PackageDetailResponse | null>(null);
  let versions = $state<PackageVersionResponse[]>([]);
  let loading = $state(true);
  let error = $state('');

  $effect(() => {
    void loadPackage();
  });

  async function loadPackage() {
    const reqOwner = owner!;
    const reqRepo = repo!;
    const reqFormat = format!;
    const reqName = name!;
    const isCurrent = latest.begin();
    loading = true;
    error = '';
    try {
      const [info, versionRes] = await Promise.all([
        packages.get(reqOwner, reqRepo, reqFormat, reqName),
        packages.getVersions(reqOwner, reqRepo, reqFormat, reqName),
      ]);
      if (!isCurrent()) return;
      packageInfo = info;
      versions = versionRes.versions || [];
    } catch (e) {
      if (!isCurrent()) return;
      error = toErrorMessage(e, t('common.unknown_error'));
    } finally {
      if (isCurrent()) {
        loading = false;
      }
    }
  }
</script>

<svelte:head>
  <title>{name} · {packageFormatLabel(format!)} · {owner}/{repo} · IronForge</title>
</svelte:head>

{#if error}
  <ErrorBanner message={error} />
{/if}

{#if loading}
  <LoadingState />
{:else if !packageInfo}
  <EmptyState title={t('packages.no_packages')} />
{:else}
  <div class="package-detail">
    <div class="package-header">
      <h1>{packageInfo.name}</h1>
      <span class="download-count">{packageInfo.download_count}</span>
    </div>

    {#if packageInfo.description}
      <p class="package-desc">{packageInfo.description}</p>
    {/if}

    <PackageVersions
      owner={owner!}
      repo={repo!}
      format={format!}
      name={name!}
      packageName={packageInfo.name}
      versions={versions}
      onRefresh={loadPackage}
    />
  </div>
{/if}

<style>

  .package-detail {
    display: flex;
    flex-direction: column;
    gap: 24px;
  }

  .package-header {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
  }

  .download-count {
    padding: 2px 8px;
    border-radius: 10px;
    font-size: 12px;
    font-weight: 600;
    background: var(--bg-tertiary);
    color: var(--text-secondary);
  }

  .package-desc {
    font-size: 14px;
    color: var(--text-secondary);
    line-height: 1.6;
    margin: 0;
  }

</style>
