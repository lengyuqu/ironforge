<script lang="ts">
  import EmptyState from '$lib/components/EmptyState.svelte';
  import type { PackageSummaryResponse } from '$lib/api/client.svelte';
  import { createT } from '$lib/i18n';
  import {
    packageFormatLabel,
    packageFormatSupportLabel,
    packageFormatUsesGenericFallback,
  } from '$lib/packageFormats';

  const t = createT();

  let {
    owner,
    repo,
    packages,
    currentPage,
    totalPages,
    onPageChange,
  }: {
    owner: string;
    repo: string;
    packages: PackageSummaryResponse[];
    currentPage: number;
    totalPages: number;
    onPageChange: (page: number) => void;
  } = $props();

  function encodePackageRouteName(name: string): string {
    return name.split('/').map(encodeURIComponent).join('/');
  }

  function packageHref(pkg: PackageSummaryResponse): string {
    return `/${owner}/${repo}/packages/${encodeURIComponent(pkg.format || '')}/${encodePackageRouteName(pkg.name)}`;
  }
</script>

{#if packages.length === 0}
  <EmptyState title={t('packages.no_packages')} />
{:else}
  <div class="package-list">
    {#each packages as pkg (pkg.id)}
      <div class="package-card surface">
        <div class="package-header">
          <a href={packageHref(pkg)} class="package-name">{pkg.name}</a>
          <span
            class="format-badge"
            class:fallback={packageFormatUsesGenericFallback(pkg.format || '')}
            title={packageFormatSupportLabel(pkg.format || '')}
          >
            {packageFormatLabel(pkg.format || '')}
          </span>
        </div>
        {#if pkg.description}
          <p class="package-desc">{pkg.description}</p>
        {/if}
        <div class="package-meta">
          <span class="version">{t('packages.version')}: {pkg.latest_version}</span>
        </div>
      </div>
    {/each}
  </div>

  {#if totalPages > 1}
    <div class="pagination">
      <button
        class="btn-outline"
        disabled={currentPage <= 1}
        onclick={() => onPageChange(currentPage - 1)}
      >
        {t('common.previous') || 'Previous'}
      </button>
      <span class="page-info">Page {currentPage} of {totalPages}</span>
      <button
        class="btn-outline"
        disabled={currentPage >= totalPages}
        onclick={() => onPageChange(currentPage + 1)}
      >
        {t('common.next') || 'Next'}
      </button>
    </div>
  {/if}
{/if}

<style>
  .package-list {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
    gap: 16px;
  }

  .package-card {
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .package-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    flex-wrap: wrap;
  }

  .package-name {
    font-size: 15px;
    font-weight: 600;
    color: var(--accent);
    text-decoration: none;
  }

  .package-name:hover {
    text-decoration: underline;
  }

  .format-badge {
    padding: 2px 8px;
    border-radius: 10px;
    font-size: 11px;
    font-weight: 600;
    background: var(--accent-subtle);
    color: var(--accent);
    white-space: nowrap;
  }

  .format-badge.fallback {
    background: var(--bg-tertiary);
    color: var(--text-secondary);
  }

  .package-desc {
    font-size: 13px;
    color: var(--text-secondary);
    margin: 0;
    line-height: 1.5;
    overflow-wrap: anywhere;
  }

  .package-meta {
    display: flex;
    gap: 16px;
    font-size: 12px;
    color: var(--text-muted);
    flex-wrap: wrap;
  }

  .pagination {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 16px;
    margin-top: 24px;
  }

  .page-info {
    font-size: 13px;
    color: var(--text-secondary);
  }
</style>
