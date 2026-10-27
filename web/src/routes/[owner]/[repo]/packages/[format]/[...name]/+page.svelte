<script lang="ts">
  import LoadingState from '$lib/components/LoadingState.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import { page } from '$app/stores';
  import RepoHeader from '$lib/components/RepoHeader.svelte';
  import {
    packages,
    type PackageDetailResponse,
    type PackageVersionResponse,
  } from '$lib/api/client.svelte';
  import PackageVersions from '$lib/components/packages/PackageVersions.svelte';
  import { createT } from '$lib/i18n';
  import { packageFormatLabel } from '$lib/packageFormats';

  const t = createT();

  let owner = $derived($page.params.owner!);
  let repo = $derived($page.params.repo!);
  let format = $derived($page.params.format!);
  let name = $derived($page.params.name!);

  let packageInfo = $state<PackageDetailResponse | null>(null);
  let versions = $state<PackageVersionResponse[]>([]);
  let loading = $state(true);
  let error = $state('');

  $effect(() => {
    loadPackage();
  });

  async function loadPackage() {
    loading = true;
    error = '';
    try {
      const [info, versionRes] = await Promise.all([
        packages.get(owner!, repo!, format!, name!),
        packages.getVersions(owner!, repo!, format!, name!),
      ]);
      packageInfo = info;
      versions = versionRes.versions || [];
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }
</script>

<svelte:head>
  <title>{name} · {packageFormatLabel(format!)} · {owner}/{repo} · IronForge</title>
</svelte:head>

<div class="page-container">
  <RepoHeader owner={owner!} repo={repo!} activeTab="packages" />

  {#if error}
    <div class="error-banner">{error}</div>
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
</div>

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

  h1 {
    font-size: 24px;
    font-weight: 600;
  }

  .download-count {
    padding: 2px 8px;
    border-radius: 10px;
    font-size: 12px;
    font-weight: 600;
    background: var(--bg-tertiary, #e5e7eb);
    color: var(--text-secondary);
  }

  .package-desc {
    font-size: 14px;
    color: var(--text-secondary);
    line-height: 1.6;
    margin: 0;
  }

  .error-banner {
    color: #f85149;
    background: rgba(248, 81, 73, 0.1);
    padding: 10px 12px;
    border-radius: var(--radius);
    margin-bottom: 16px;
  }
</style>
