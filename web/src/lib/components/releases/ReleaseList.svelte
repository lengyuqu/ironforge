<script lang="ts">
  // Release list — pure presentation: renders release cards and pagination,
  // delegating reload intents to the parent.
  import type { Release } from '$lib/types/entities';
  import type { ReleaseAsset } from '$lib/api/releases';
  import ReleaseCard from './ReleaseCard.svelte';
  import { createT } from '$lib/i18n';

  const t = createT();

  interface Props {
    owner: string;
    repo: string;
    releases: Release[];
    assets: Record<number, ReleaseAsset[]>;
    currentPage: number;
    totalPages: number;
    onReload: () => void;
    onPageChange: (page: number) => void;
  }

  let { owner, repo, releases, assets, currentPage, totalPages, onReload, onPageChange }: Props =
    $props();
</script>

<div class="release-list">
  {#each releases as release, index (release.id)}
    <ReleaseCard
      {owner}
      {repo}
      release={release}
      assets={assets[release.id] || []}
      isLatest={index === 0}
      onChanged={onReload}
    />
  {/each}
</div>

{#if totalPages > 1}
  <div class="pagination">
    <button
      class="btn-outline"
      disabled={currentPage <= 1}
      onclick={() => onPageChange(currentPage - 1)}
    >
      {t('common.previous')}
    </button>
    <span class="page-info">{t('common.page_of', { page: currentPage, total: totalPages })}</span>
    <button
      class="btn-outline"
      disabled={currentPage >= totalPages}
      onclick={() => onPageChange(currentPage + 1)}
    >
      {t('common.next')}
    </button>
  </div>
{/if}

<style>
  .release-list {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .pagination {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 16px;
    margin-top: 24px;
  }

  .page-info {
    font-size: 14px;
    color: var(--text-secondary);
  }
</style>
