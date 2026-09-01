<script lang="ts">
  // Release list — pure presentation: renders release cards and pagination,
  // delegating reload intents to the parent.
  import type { Release } from '$lib/types/entities';
  import type { ReleaseAsset } from '$lib/api/releases';
  import ReleaseCard from './ReleaseCard.svelte';

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
      Previous
    </button>
    <span class="page-info">Page {currentPage} of {totalPages}</span>
    <button
      class="btn-outline"
      disabled={currentPage >= totalPages}
      onclick={() => onPageChange(currentPage + 1)}
    >
      Next
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

  .btn-outline {
    padding: 5px 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text-primary);
    font-size: 13px;
    cursor: pointer;
  }
  .btn-outline:hover { background: var(--bg-hover); }
  .btn-outline:disabled { opacity: 0.5; cursor: not-allowed; }
</style>
